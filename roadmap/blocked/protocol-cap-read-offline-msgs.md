---
id: protocol-cap-read-offline-msgs
title: Drain kept messages over ReadOfflineMsgs where the answer capabilities exist
topic: protocol
status: blocked
origin: gridspec-instant-messages follow-up (2026-10-14)
refs: [viewer-offline-im-drain, gridspec-instant-messages,
  test-offline-msg-fetch, test-offline-msg-fetch-aditi]
blocked_by: [protocol-cap-offline-friendship-answers,
  protocol-cap-offline-group-invite-answers]
---

Context: [context/protocol.md](../context/protocol.md).

Firestorm (and the Linden viewer it follows) reads the messages kept while
the agent was away over `ReadOfflineMsgs` by default
(`LLIMProcessing::requestOfflineMessages`, `llimprocessing.cpp`; Firestorm's
`FSUseReadOfflineMsgsCap` defaults to on). It falls back to UDP
`RetrieveInstantMessages` when the region lacks `ReadOfflineMsgs`,
`AcceptFriendship` or `AcceptGroupInvite`, and when the GET fails. The
capability's records carry a nil `transaction-id`, so a friendship offer or
group invitation read this way can only be answered over the answer
capabilities — hence the two blockers.

Our viewer drains over UDP on purpose ([[viewer-offline-im-drain]]). The
client's decoder already reads Second Life's nested-array answer and takes the
id from `asset_id` for message, auto-response and object dialogs
([[gridspec-instant-messages]], `book/src/gridspec/chat.md` § Instant
messages); on aditi either road empties the store for both.

Switch the viewer's login drain to `ReadOfflineMsgs` under Firestorm's
conditions (all three capabilities granted), fall back to UDP on a missing
capability or a failed GET, and mark offers and invitations that arrived
this way so their answers go over the capabilities rather than UDP. Check
the fake grid serves `ReadOfflineMsgs` per flavour as `seed-capabilities`
measured it, and that the drain still delivers once and only once on both
grids (`instant-messages` away rounds, `offline-msg-fetch`).
