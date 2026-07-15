---
id: viewer-rlv-notify
title: RLV — @notify broadcast on restriction changes
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-rlva-enforcement
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

Implement `@notify:<channel>;<filter>=add` — an object asking to be **told
whenever any restriction changes**. This means every state transition in
[[viewer-rlv-restriction-state]] has to be broadcast, not merely applied: when a
restriction is added, lifted, or cleared, each registered notify subscription
whose filter matches emits a chat line on its channel describing the change.

The work is a subscription registry (channel + filter string, per object) hung
off the state machine's transition events, plus the message formatting the
reference uses so scripts parse the notifications they expect. Because it
observes *every* transition, wire it into the state machine's change hook rather
than sprinkling emit calls across the enforcement families.

Reference (Firestorm, read-only): `rlvhandler.cpp` (the `@notify` subscription
list and its emit-on-change hook), `rlvcommon.cpp`.
