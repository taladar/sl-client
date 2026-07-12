---
id: protocol-28
title: Complete the IM surface (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**28. Complete the IM surface (done) ✅ — `ImprovedInstantMessage` offer/session
flows, `StartLure`/`TeleportLureRequest`, `RetrieveInstantMessages`,
`ReadOfflineMsgs` CAPS · 8 pts. (extends #2, Tier A.)** Item #2 implemented 1:1
IM send/receive and surfaced every inbound `ImDialog` sub-type, but several
reply/send flows were deferred; this finishes them. Implemented:
**teleport offer/lure** — `offer_teleport` (`StartLure`), `accept_teleport_lure`
(`TeleportLureRequest` with `TELEPORT_FLAGS_VIA_LURE`, driving the existing
teleport handover; the lure id's encoded region handle is parsed via OpenSim's
`BuildFakeParcelID` layout), `decline_teleport_lure` (`IM_LURE_DECLINED`), and
`request_teleport` (`IM_TELEPORT_REQUEST`). **Inventory offers** —
`give_inventory` / `give_inventory_folder` (`IM_INVENTORY_OFFERED` with the
`[asset-type byte] ++ [16-byte id]` binary bucket, a new `AssetType::Folder`
leading a folder offer), and `accept_inventory_offer` /
`decline_inventory_offer` (`IM_INVENTORY_ACCEPTED` / `_DECLINED`, the bucket
carrying the destination / trash folder id). Incoming offers decode via a new
`InstantMessage::inventory_offer` → `InventoryOffer` value type (asset type,
item id, transaction id, sender, task-vs-agent).
**Conference / ad-hoc sessions** — `start_conference`
(`IM_SESSION_CONFERENCE_START`, invitee ids packed in the bucket; call again to
add invitees), `send_conference_message` (`IM_SESSION_SEND`), `leave_conference`
(`IM_SESSION_LEAVE`), with incoming traffic surfaced as
`Event::Conference{SessionMessage,SessionParticipant,Invited}` (the
`from_group`-clear siblings of #7's group-session events; the modern CAPS
`ChatterBoxInvitation` is decoded too). **Offline-IM history** — the legacy
`retrieve_instant_messages` (`RetrieveInstantMessages` UDP, replies re-delivered
as offline `Event::InstantMessageReceived`) plus the modern `ReadOfflineMsgs`
capability (added to the seed; GET decoded by `handle_caps_event` into one
offline `Event::InstantMessageReceived` per stored record). All wired as
`Command`/`SlCommand` variants through both runtimes. Field values and the
binary-bucket layouts were cross-checked against the Firestorm viewer
(`llgiveinventory.cpp`, `llviewermessage.cpp`, `llavataractions.cpp`,
`llimview.cpp`, `llteleportflags.h`) and OpenSim's `LureModule` /
`InventoryTransferModule` / `OfflineMessageModule`. Covered by thirteen
`lifecycle.rs` tests (the lure offer/accept/decline encodings, give-item and
give-folder buckets, the inventory-offer decode + accept/decline round-trip, the
conference start/send/leave encodings and inbound decode, the
`RetrieveInstantMessages` trigger, the `ReadOfflineMsgs` array decode, and the
`ChatterBoxInvitation` decode). *Live-verified against the local OpenSim with
two accounts (Avatar Tester + Friend Tester): A offered B a teleport (B received
the `LureUser` IM and declined), and A gave B a worn body-part item — B received
the `InventoryOffered` IM (OpenSim having rewritten the session id to the copy
id, as the viewer expects), accepted it into B's inventory root, and the
`InventoryAccepted` reply round-tripped back to A. The offline-IM and conference
caps are SL-shaped (stock OpenSim's `InstantMessageModule` handles only 1:1 IMs,
and `ReadOfflineMsgs` is absent), so those are unit-tested only and the commands
no-op cleanly on OpenSim. Test: local OpenSim — two accounts for the offer
round-trips; the grid's offline-IM module plus an offline-then-relogin test for
UDP history.*
