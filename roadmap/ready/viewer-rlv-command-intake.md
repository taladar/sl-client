---
id: viewer-rlv-command-intake
title: RLV — the owner-say command intake and the reply/notify chat path
topic: viewer
status: ready
origin: gap found wiring viewer-rlv-debug-settings-commands (2026-09-07)
refs: [viewer-rlv-command-parser, viewer-rlv-queries, viewer-rlv-notify,
  viewer-rlv-debug-settings-commands, viewer-rlva-floaters-toggles]
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

**Nothing an object says reaches the RLV engine yet.** The whole engine is
built — the parser, the state machine, the locks, the queries, the enforcement
façade both ways, the extension commands — and the only thing that has ever
fed it a command is the RLVa console, with the agent itself as the issuer. A
worn collar cannot restrain this viewer, because the door is not cut.

The door has two halves, and every `sl-rlv` task so far has had to write "not
verified live: no consumer yet" because neither exists.

## In: the owner-say gate

Route arriving chat into [[viewer-rlv-command-parser]] under the reference's
own conditions, and swallow what it takes:

- the chat is `CHAT_TYPE_OWNER` on channel `0` (`llOwnerSay` / `llRegionSayTo`),
  which is what makes it an object and not an avatar;
- the line starts with `@` (`sl_rlv::is_rlv_line`), and is then *not* shown in
  the chat log — the viewer eats it;
- each command is applied with the issuing object's key, so
  `RlvState::clear_object` on detach lifts exactly what that object held. That
  key is also what the attachment bookkeeping
  (`RlvState::set_object_attachment`) needs, so the Restrictions floater can
  say where a restriction came from;
- `RestrainedLoveDebug` echoes every processed command into the RLVa console,
  which is the setting's whole purpose and today has nothing to echo
  ([[viewer-rlva-floaters-toggles]]).

### Who is allowed to speak — read this before tightening the gate

The obvious guess is that RLV only listens to **attachments**, and that an
in-world object has to go through a relay. That is the *content* ecosystem's
shape, not the viewer's rule, and building the gate from the guess would break
every in-world RLV device. What the reference actually tests
(`llviewermessage.cpp:3142`) is:

```text
RLV enabled && message starts with '@' && length > 3 && CHAT_TYPE_OWNER &&
    ( no chatter object || !isAttachment || !isTempAttachment
      || RLVaEnableTemporaryAttachments )
```

That trailing clause is an **or**-chain, so it excludes exactly one case: a
*temporary* attachment while `RLVaEnableTemporaryAttachments` is off. A rezzed
in-world prim is not excluded at all — a bed, a cage or a cuff-post the agent
owns commands the viewer directly, and always has.

**The security boundary is ownership, and the simulator draws it, not the
viewer.** `CHAT_TYPE_OWNER` is what `llOwnerSay` produces, and the simulator
delivers it only to the object's owner. Somebody else's furniture physically
cannot reach this agent's chat as owner-say, so there is nothing for the viewer
to check — which is why the reference does not re-check ownership here, and why
this task must not invent a check that pretends to add safety.

**That ownership boundary is exactly why relays exist.** A club's poseball is
owned by the club, so it cannot speak RLV to a visitor. Instead it asks a
scripted **relay** the visitor wears — which the visitor owns — on the RLV
relay channel (`-1812221819`), and the relay re-issues the commands as its
own `llOwnerSay`. The consent (ask / auto / off, per-object permission) lives
in the relay script, where the user installed it. All of that is content: the
viewer only ever sees the relay's own owner-say and cannot tell it from any
other worn object, so **there is nothing to implement here for relays** — but
the gate must stay loose enough that a relay, and an owned in-world object,
both get through.

Two things the reference does gate on, which this task does own:

- **blocked objects** (`RlvHandler::isBlockedObject`, `m_BlockedObjects`): a
  temporary attachment that could not be resolved is remembered by name, and
  every command from it is refused with `RLV_RET_FAILED_BLOCKED` — except a
  remove or a `@clear`, which are always let through so a block can never
  strand a restriction. `RlvOutcome` has no variant for this yet.
- **expiry for something that never detaches.** An in-world object has no
  detach event to fire `RlvState::clear_object`, so the reference garbage
  collects every 30 seconds: an object that once existed in the object list and
  now does not is cleared at once, and one that never resolved is cleared after
  20 misses (about ten minutes). Without this, a prim that was derezzed or left
  behind in another region restrains the agent forever — which is the one
  failure mode of this whole family a user cannot get out of.

## Out: the chat-back path

Three producers build a line to chat and nothing sends it:

- `RlvState::answer` — the `@get*` reply, **shouted** on the channel the query
  named ([[viewer-rlv-queries]]);
- `RlvState::take_notifications` — the `@notify` subscribers' lines
  ([[viewer-rlv-notify]]);
- `RlvState::run_extension` — the `@getdebug_*` answer
  ([[viewer-rlv-debug-settings-commands]]).

All three hand back an `RlvReply { channel, message }`, already truncated and
channel-checked; what is missing is the one system that drains them into the
session's chat send. It is one seam for all three, which is why it belongs
here rather than in each of them.

Doing this is also what makes every `RlvQuerySource` / `RlvActionSource` /
`RlvExtSource` implementation *observable*: today they are exercised only by
unit tests and the console, and the "not verified live" note on half the RLV
family clears the moment a scripted object can talk to the viewer.

Reference (Firestorm, read-only): the chat hook in `llviewermessage.cpp`
(~L3140, the `CHAT_TYPE_OWNER` case), `RlvHandler::processCommand` and the
garbage collector / blocked-object list in `rlvhandler.cpp`,
`RlvUtil::sendChatReply` in `rlvcommon.cpp`.
