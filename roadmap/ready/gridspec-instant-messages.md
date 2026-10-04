---
id: gridspec-instant-messages
title: Instant messages, typing, busy replies and offline storage on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-offline-im-drain, server-message-routing]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

`im-1to1` green on both; OpenSim confirms storage with "Message saved.", SL
"User not online - message will be stored..."; SL's read-back stayed empty
(IM-to-email suspected). The fake grid relays nothing.

## Discover

`im_1to1`, `im_typing`, `offline_msg_fetch` (with IM-to-email off on aditi),
busy auto-response, IM to unknown / muted agents, `MessageFromObject`; two
avatars and the two-viewer live tests.

## Document

`book/src/gridspec/chat.md` § Instant messages.

## Fake grid

Large — [[server-fake-grid-im-relay]].

## Viewer

No wait for acceptance IMs on SL; de-duplicate SL's offline replay.

## Capabilities done in this task

The `ReadOfflineMsgs` half of [[protocol-cap-offline-friendship-answers]]:
when all of `ReadOfflineMsgs`, `AcceptFriendship` and `AcceptGroupInvite` are
granted, offline messages arrive over the capability, as Firestorm reads them.
Measure both deliveries (shape, transaction ids) on aditi.
