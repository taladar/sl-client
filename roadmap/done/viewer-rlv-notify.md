---
id: viewer-rlv-notify
title: RLV — @notify broadcast on restriction changes
topic: viewer
status: done
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

## Done (2026-09-06)

`sl-rlv/src/notify.rs` holds the subscription registry and the
`RlvNotification { channel, message }` the consumer chats; `RlvState` owns
one, fills it from the `@notify` option arm, and queues lines for
`RlvState::take_notifications`. Four reference decisions the tests pin:

- the broadcast hangs off the **command**, at one choke point in `apply` and
  `clear`, so a command that *failed* is reported too — RLV reports invalid
  commands, and a script that stopped hearing them would read that as a viewer
  gone deaf;
- the filter is matched against the `behaviour[:option]` half only, never the
  `=n` glued on after it, so one subscription hears a restriction go on *and*
  off;
- `@clear` reports itself and not the restrictions it lifted (the reference
  lifts those with internal commands its notify hook skips), and a detach is a
  `@clear` — so a detaching object announces a clear it no longer hears, while
  everyone else does;
- the param is echoed as the object spelled it (`=add` stays `add`), which
  needed the raw param text keeping on `RlvCommand`.

Not verified live: no consumer yet. The chat send (a shout on the channel,
truncated at the 1023-byte chat limit) is the viewer's half, and arrives with
[[viewer-rlv-queries]], which needs the same reply path.
