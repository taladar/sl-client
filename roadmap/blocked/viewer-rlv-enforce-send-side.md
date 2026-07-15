---
id: viewer-rlv-enforce-send-side
title: RLV — enforce send-side blocks at the Session boundary
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-rlva-enforcement
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

Refuse the forbidden **outgoing** commands — the session, not the renderer. This
is the family that a **headless** `sl-client` bot must honour too, which is the
argument for putting the state model in a crate
([[viewer-rlv-restriction-state]]) and the choke points at the command boundary
rather than in Bevy systems. An
RLV-compliant viewer **must not offer a bypass**, so the check belongs at the
lowest choke point available — the `Session` command surface — never
re-implemented per call site.

The behaviours (`ERlvBehaviour`) each map to a command `Session` (or the
viewer's input path) must refuse to issue:

- chat: `@sendchat`, `@sendim` / `@sendimto`, `@sendchannel`,
  `@chatshout` / `@chatnormal` / `@chatwhisper`, `@emote`;
- teleport: `@tplm` / `@tploc` / `@tplure` / `@tprequest`;
- posture and attachments: `@sit` / `@unsit`,
  `@detach` / `@remoutfit` / `@addattach`;
- world interaction: `@rez`, `@edit`, `@touchall`, `@fly`, `@setgroup`, …

Mirror the reference façade shape exactly: a restriction is asked about at the
choke point via one predicate (`RlvActions::canX()` / `hasBehaviour()`), called
from all over `llviewer*`. Copy that — ask [[viewer-rlv-restriction-state]] at
the choke point.

Reference (Firestorm, read-only): `rlvactions.h` (`RlvActions::canX()` /
`hasBehaviour()`), `rlvhandler.cpp`.
