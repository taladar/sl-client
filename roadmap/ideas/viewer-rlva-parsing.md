---
id: viewer-rlva-parsing
title: RLVa — parse the restriction protocol (the @-command language)
topic: viewer
status: ideas
origin: user request (2026-07)
refs: [viewer-rlva-enforcement]
---

Context: [context/viewer.md](../context/viewer.md).

**RLV/RLVa is a chat-based control protocol between a worn attachment and the
viewer**: the object `llOwnerSay`s commands, the viewer obeys them (restricting
what its user can do) and answers queries on a chat channel. It is what every
collar, HUD, furniture and adult-content system in Second Life is built on, so a
viewer without it cannot wear most of the content people actually own. This task
is the **language half** — parse, model, and track the commands; obeying them is
[[viewer-rlva-enforcement]].

The wire form (`llviewermessage.cpp` ~3141, `rlvhandler.cpp`, `rlvdefines.h`):

- The carrier is ordinary **owner-say chat** (`CHAT_TYPE_OWNER` on channel 0)
  from an object the agent owns — nothing new on the wire, which is why it works
  on any grid. A message qualifies when it starts with `@` (`RLV_CMD_PREFIX`).
  The viewer *swallows* it: it never appears in the chat log.
- The payload is a **comma-separated list** of commands, each
  `behaviour[:option]=param`, lower-cased. The `param` chooses the kind
  (`ERlvParamType`): `n`/`add` adds a restriction, `y`/`rem` lifts it, `force`
  performs an action (`@sit:<uuid>=force`, `@remoutfit=force`), and a **number**
  makes it a query whose answer is chatted back on that channel
  (`@version=2222`, `@getoutfit=1234`). Plus `@clear` (with an optional filter).
- Restrictions are **per issuing object** and reference-counted across objects:
  a behaviour stays in force while *any* object holds it, and every restriction
  an object placed is dropped when it detaches (`rlvhandler.cpp` clears on
  detach). That bookkeeping — object → behaviours, behaviour → objects, plus
  per-behaviour *exceptions* (`@sendim:<uuid>=add`) — is the heart of the state
  model.
- ~150 behaviours (`ERlvBehaviour` in `rlvdefines.h`), a version handshake
  (`@version` / `@versionnew` / `@versionnum`; RLVa reports 3.4.3 with a 2.9.28
  compatibility floor), and an IM-carried query path (`processIMQuery`).

Shape of the work: a **pure crate** (`sl-rlv`, no I/O, in the spirit of
`sl-prim` / `sl-anim`) that decodes a chat line into typed commands, holds the
restriction/exception state machine keyed by issuing object, and answers the
queries that need no viewer state. Everything it decides is data the viewer (and
any bot on `sl-client`) can then act on. It should be usable *without* the Bevy
viewer: a headless client wanting to be RLV-compliant needs exactly this.

Note the deliberate split: parsing and state are grid-agnostic and unit-testable
to the letter (the reference's own `rlvfloaters.cpp` debug console feeds
hand-typed commands through the same path, and that is precisely how this crate
should be tested). Enforcement is where the viewer's own surfaces get involved,
and that is a separate, much larger job — see [[viewer-rlva-enforcement]].
