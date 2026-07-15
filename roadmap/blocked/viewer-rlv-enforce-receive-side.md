---
id: viewer-rlv-enforce-receive-side
title: RLV — receive-side chat/IM filters and redirect
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-rlva-enforcement
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

Filter and redirect the **incoming/outgoing chat pipeline** according to the
restriction state ([[viewer-rlv-restriction-state]]). The chat pipeline rewrites
or drops messages, with **per-avatar exceptions**:

- receive filters: `@recvchat` / `@recvchatfrom`, `@recvim` / `@recvimfrom`,
  `@recvemote` — drop or hold what arrives from a blocked source, letting
  through the avatars named as exceptions;
- redirect: `@redirchat` / `@rediremote` — re-route what *you* say to a
  channel instead of open chat.

These sit on the message path (chat overlay + IM), consulting the state machine
per message and honouring the per-avatar exception sets it tracks. Keep the
check at the single filter chokepoint the reference uses so no receive path can
skip it.

Reference (Firestorm, read-only): `rlvhandler.cpp` (receive filters, redirect),
`rlvactions.h`.
