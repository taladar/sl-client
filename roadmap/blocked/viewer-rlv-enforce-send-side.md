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

## Parity-audit addendum (2026-08-19)

The audit's command-by-command mapping puts the following send-side
dictionary commands in this task's scope beyond the subset the body
names: the movement family `@jump`, `@alwaysrun`, `@temprun`; economy
`@buy`, `@pay`, `@share` (with `_sec`); the `@interact` blanket block;
the full touch granularity `@touchworld`, `@touchthis`, `@touchme`,
`@touchattach`, `@touchattachself`, `@touchattachother`, `@touchhud`,
and `@fartouch` (plus its `@touchfar` synonym) with the FARTOUCHDIST
distance modifier; `@sendgesture`; `@sittp`, `@standtp` and `@tplocal`
with the SITTPDIST / TPLOCALDIST distance modifiers;
`@sendchannel_except`; and the sendim / startim distance min/max
modifiers (SENDIMDISTMIN/MAX, STARTIMDISTMIN/MAX). The typed modifier
slots themselves come from [[viewer-rlv-restriction-state]].
