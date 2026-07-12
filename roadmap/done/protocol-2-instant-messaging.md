---
id: protocol-2
title: Instant messaging
topic: protocol
status: done
origin: ROADMAP.md
---

Context: [context/protocol.md](../context/protocol.md).

**2. Instant messaging — `ImprovedInstantMessage` · 5 pts. ✅ Done.**
Implemented: `Session::send_instant_message` (1:1, with the canonical
`agent_id XOR target` session id) and `Session::send_im_typing`, plus an
`ImDialog` enum classifying the dialog sub-types multiplexed over this message
(inventory offers, teleport offers/lures, group invites, friendship offers,
object IMs, group/conference messages, …). Incoming IMs surface as
`Event::InstantMessageReceived` (a full `InstantMessage`: sender, dialog, ids,
region/position, offline flag, message, binary bucket) with typing split out as
a distinct `Event::ImTyping`. Wired as
`Command::InstantMessage`/`Command::ImTyping` through both runtimes; verified
live against the local OpenSim (self-IM round-trip for both message and typing).
Incoming offline IMs that the sim pushes as `ImprovedInstantMessage` are already
surfaced (with `offline = true`). Deferred follow-ups: (a) offer accept/decline
reply flows (inventory/teleport/friendship); (b) offline-IM *history retrieval*
— the legacy `RetrieveInstantMessages` UDP trigger and the modern SL
`ReadOfflineMsgs` CAPS path (needs the grid's offline module enabled, and an
offline-then-relogin test); (c) sending into group/conference sessions and the
session start/invite/leave dialogs (`IM_SESSION_*`), which belong with #7 (group
support). *Test: local OpenSim (single account suffices via a self-IM;
cross-avatar needs two).*
