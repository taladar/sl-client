---
id: viewer-offline-im-drain
title: Drain stored offline instant messages at login
topic: viewer
status: done
origin: user request (2026-07-29), fallout of viewer-group-notice-display — an
  offline notice/IM must be delivered once at login, not held forever
refs: [viewer-group-notice-display, viewer-dialog-offers-invites]
---

Context: [context/viewer.md](../context/viewer.md).

The simulator stores instant messages (1:1 IMs, inventory / teleport offers,
friendship / group invites, **group notices**) sent while the agent is offline,
and delivers them only when the viewer **asks** — the reference
`LLIMProcessing::requestOfflineMessages`, once, after login. Until then OpenSim
holds them (and re-holds on every login); on retrieval it hands them over and
**deletes** them (one-shot). Our viewer never sent that request, so genuinely
offline messages were never delivered.

## Done (2026-07-29)

- `drive_session` sends `Command::RetrieveInstantMessages` **once** on the first
  `RegionHandshakeComplete` (guarded by
  `ViewerSession::offline_messages_requested` so a region cross does not
  re-drain). The stored messages arrive as ordinary `InstantMessageReceived`
  events with `offline` set and fold into the conversations / offers /
  group-notice surfaces like any live IM.
- Uses the **legacy UDP** `RetrieveInstantMessages`, not the modern
  `ReadOfflineMsgs` cap: the UDP delivery carries the per-message transaction
  ids our UDP accept paths need, whereas the cap path drops them — which is
  exactly why the reference only prefers the cap when the `AcceptFriendship` /
  `AcceptGroupInvite` caps are also present (a cap-accept path we do not wire
  yet). When those land, revisit to prefer the cap on Second Life.

Interaction with [[viewer-notification-persistence]]: an offline group notice is
delivered once (server drains + deletes), shown, and client-persisted; the
client store then re-displays it across relogs until answered — no
double-display, because the server has already deleted its copy.

Reference (Firestorm, read-only): `LLIMProcessing::requestOfflineMessages` /
`requestOfflineMessagesLegacy`, called from `LLAppViewer::idle`.
