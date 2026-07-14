---
id: viewer-rlva-enforcement
title: RLVa — enforce the restrictions (and answer the queries)
topic: viewer
status: ideas
origin: user request (2026-07)
refs: [viewer-rlva-parsing]
blocked_by: [viewer-rlva-parsing]
---

Context: [context/viewer.md](../context/viewer.md).

Given the parsed command stream and restriction state of
[[viewer-rlva-parsing]], **actually obey it**. This is the large half: RLVa
touches nearly every surface a viewer has, because a restriction is only real if
*every* path that could do the forbidden thing checks it. In the reference the
checks live behind one façade — `RlvActions::canX()` / `hasBehaviour()`
(`rlvactions.h`), called from all over `llviewer*` — and that shape is worth
copying exactly: a restriction is asked about at the choke point, never
re-implemented per call site.

The ~150 behaviours (`ERlvBehaviour`) group into a handful of enforcement
families, and each maps onto a part of this workspace that already exists:

- **Send-side blocks — the session, not the renderer.** `@sendchat`,
  `@sendim`/`@sendimto`, `@sendchannel`,
  `@chatshout`/`@chatnormal`/`@chatwhisper`, `@emote`,
  `@tplm`/`@tploc`/`@tplure`/`@tprequest`, `@sit`/`@unsit`,
  `@detach`/`@remoutfit`/`@addattach`, `@rez`, `@edit`, `@touchall`, `@fly`,
  `@setgroup`, … Each is a command `Session` (or the viewer's input path) must
  refuse to issue. These are the ones a **headless** `sl-client` bot must honour
  too, which is the argument for putting the state model in a crate and the
  choke points at the command boundary rather than in Bevy systems.
- **Receive-side filters.** `@recvchat`/`@recvchatfrom`,
  `@recvim`/`@recvimfrom`, `@recvemote`, `@redirchat`/`@rediremote` (re-route
  what you say to a channel): the incoming/outgoing chat pipeline rewrites or
  drops messages, with per-avatar exceptions.
- **Information hiding — the viewer's own display.** `@shownames` /
  `@shownametags` (obfuscate every name, including in chat and the mini-map —
  the reference has a whole anonymisation layer, `RlvUtil::filterNames`),
  `@showloc` (hide the region/parcel name), `@showminimap` / `@showworldmap`,
  `@showinv`, `@showhovertext*`, `@showself` / `@showselfhead`. These need the
  viewer's chat overlay, name tags and (future) UI to route their text through
  one filter.
- **Camera and vision.** `@setcam_*` (fov, distance, avatar-locked view),
  `@camtextures`, and RLVa's render effects (`rlveffects.cpp`: the vision sphere
  / blur overlay) — which land squarely on the Phase 22–33 rendering work and
  the camera module.
- **Forced actions.** `@sit:<uuid>=force`, `@unsit=force`,
  `@tpto:<x>/<y>/<z>=force`, `@remoutfit=force`, `@attach:<path>=force` against
  the **shared `#RLV` inventory folder** (a whole sub-protocol of its own: a
  folder tree the object addresses by path, `@getinvworn`, `@findfolder`, …).
  This one leans on the inventory work.
- **Queries.** `@version*`, `@getoutfit`, `@getattach`, `@getstatus`,
  `@getinv`/`@getinvworn`, `@getsitid`, `@getcam_*`, … answered by chatting back
  on the given channel (`RlvUtil::sendChatReply`, split across multiple lines
  when long).
- **Notifications.** `@notify:<channel>;<filter>=add` — the object asks to be
  told whenever *any* restriction changes, which means every state transition
  has to be broadcast, not just applied.

Worth deciding early: **an RLV-compliant viewer must not offer a bypass** (that
is the whole point of the protocol, and content authors rely on it), so the
restriction checks belong at the lowest choke point available — the `Session`
command surface — rather than only in the UI that would normally send them.
Equally worth deciding: whether RLVa is on by default, since a restricted client
is a strange default for a *library*; the reference gates it behind a setting
that requires a restart.

Scope this as several tasks when it is picked up (send-side, receive-side,
information hiding, camera, forced actions + `#RLV` folder, queries, notify) —
this note is the umbrella.
