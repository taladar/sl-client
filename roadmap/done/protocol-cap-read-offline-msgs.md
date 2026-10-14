---
id: protocol-cap-read-offline-msgs
title: Drain kept messages over ReadOfflineMsgs where the answer capabilities exist
topic: protocol
status: done
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

## Done (2026-10-14)

- `Command::RequestOfflineMessages` reads over `ReadOfflineMsgs` only where
  the region grants it and both answer capabilities
  (`OFFLINE_DRAIN_CAPABILITIES`), over UDP otherwise, and again over UDP
  when the GET fails (both runtimes).
- The viewer drains with it once, after the first handshake **and** the
  region's capabilities, as the reference waits for `capabilitiesReceived`.
- The decoder folds `asset_id` back in as the reference's `aux_id`: an
  invitation's group becomes its sender, an inventory offer's transaction id
  its id, a notice's group and text bucket the UDP bucket.
- `InstantMessage::answered_over_capabilities` is the one rule for
  `use_offline_cap` (any kept friendship offer; a kept invitation without
  an id), used by the viewer and the censuses.
- The `friends-presence` and `groups` client away rounds now read as the
  viewer does and accept as it answers; on aditi the invitation and offer
  arrived over the capability without an id and were accepted through
  `AcceptFriendship` / `AcceptGroupInvite`. `instant-messages` and
  `offline-msg-fetch` pass on both grids.
- **Mutes, as the reference handles them**:
  - the drain also waits until the mute list has settled, meaning it has
    arrived, the grid answered "unchanged", the file would not parse, or 30 s
    have passed (`MuteModel::is_settled`, `LLMuteList::updateLoadState`);
  - an inventory or friendship offer from a muted sender is declined;
  - a teleport offer or request, or an invitation from a muted group or
    inviter, is ignored;
  - a muted poster's group notice is dropped (`muted_offer`,
    `notice_sender_muted`).

Differences: the reference also sends its busy reply to a muted group's
invitation, addressed to the group; none is sent here. OpenSim's notice names
the group as its sender, so the poster is matched by name against the muted
residents' entries instead of through a name cache.

Not measured: a kept inventory offer and a kept notice over the capability are
read as the reference reads them (`gridspec-inventory-offers`). The fake grid
grants `ReadOfflineMsgs` but not the answer capabilities yet. A client
therefore drains over UDP there, which the fake grid does not answer (noted in
[[server-fake-grid-friends-presence]] and [[server-fake-grid-groups]]).
