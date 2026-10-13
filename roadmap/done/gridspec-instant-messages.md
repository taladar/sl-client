---
id: gridspec-instant-messages
title: Instant messages, typing, busy replies and offline storage on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-offline-im-drain, server-message-routing,
  server-fake-grid-im-relay, protocol-cap-offline-friendship-answers,
  gridspec-friends-presence, gridspec-mute-list]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-13)

Measured and written up in `book/src/gridspec/chat.md` § Instant messages.

- **Discover.** One new two-avatar case, `instant-messages`: avatar A
  writes and owns a probe prim, avatar B is written to, mutes A for a while
  and logs out three times. Every field of a message as it arrives; text
  limits; typing; an automatic reply; a message to oneself, to an unknown id
  and to no id; a message whose every field the viewer made up; dialogs a
  viewer does not send; a burst; a script's `llInstantMessage`; all of it
  again under a mute; and six kinds of message to an avatar that is away,
  read back over `RetrieveInstantMessages`, by reading `ReadOfflineMsgs`
  raw, and as the client reads it. Held to 95 answers; nothing changed
  between two runs on either grid.
- **Findings.** Second Life rewrites a viewer's name, id, place and bucket
  and takes only its time; OpenSim passes name, id, place and bucket on as
  sent. Both cut a text at 1023 bytes. **Neither grid enforces a mute on an
  instant message**, a typing notice or a script's message. Second Life
  drops dialog 19 from a viewer, OpenSim passes it on. Second Life suspends
  a sender after fourteen messages in a second. A script's message comes
  from its owner's id with the prim as the id; OpenSim states it at the prim
  with estate 101, Second Life at zero. Second Life keeps an automatic
  reply for an avatar that is away, OpenSim refuses it. Neither replays
  anything unasked; **both serve everything over
  `RetrieveInstantMessages`**, and on Second Life either road empties the
  store for both.
- **The "empty read-back" was ours.** Second Life's `ReadOfflineMsgs`
  answers an array holding one array of records; `sl-proto` read the outer
  array's element as a record. It reads the nested form now, and takes
  `asset_id` for the id of a typed message, an automatic reply and a
  script's message, whose `transaction-id` Second Life states as nil.
- **Viewer.** The grids' "message kept" notice comes from the id of the
  resident who is away, signed "Second Life" / "System": the viewer filed it
  as that resident's line and took the signature for their legacy name. It
  is now shown under the grid's signature, linked to nobody; a notice from
  no id goes with the nearby chat; the automatic reply answers neither.
  Unit-tested in `sl-viewer-people`.

**Differences from the plan.**

- **The offline drain was not switched to `ReadOfflineMsgs`.** The plan's
  *Capabilities* section wanted it where all three capabilities are
  granted. Measured: `RetrieveInstantMessages` brings everything on Second
  Life too, with every id, while the capability states every
  `transaction-id` as nil. Switching before
  [[protocol-cap-offline-friendship-answers]] can answer an offer without
  an id would break accepting an offline friendship offer, and the client
  does not ask for `AcceptFriendship` yet, so the condition could not come
  true. The decoder is fixed and measured live
  (`Command::RequestOfflineMessages`); the switch is left to
  [[gridspec-friends-presence]].
- **Fake grid: nothing built**, as planned (large); what the relay must do
  per flavour is written into [[server-fake-grid-im-relay]], and the case
  joins the offline list there.
- "Busy auto-response" is the viewer's own reply; what was measured is what
  a grid does with dialog 20.
- "No wait for acceptance IMs on SL" and "de-duplicate SL's offline replay"
  (the plan's *Viewer* section): there is no unasked replay to
  de-duplicate, and acceptance messages belong to the offers
  ([[gridspec-friends-presence]], `gridspec-inventory-offers`).
- The two-viewer live tests were not run; the viewer change is held by a
  unit test.

**Not measured:** offers kept for an avatar that is away; how long the
suspension lasts; how many messages are kept and for how long; forwarding as
mail (off on both test accounts); avatars in different regions; a mute by
name or of an object ([[gridspec-mute-list]]).

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
