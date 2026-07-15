---
id: viewer-rlv-command-parser
title: RLV — parse the @-command chat protocol
topic: viewer
status: ready
origin: user request (2026-07); split from viewer-rlva-parsing
refs: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

RLV/RLVa is a chat-based control protocol between a worn attachment and the
viewer: the object `llOwnerSay`s commands and the viewer obeys them. This task
is the **language decoder** — turn a chat line into typed commands. Obeying
them is the restriction state ([[viewer-rlv-restriction-state]]) and the
enforcement families that build on it.

The wire form (`rlvhandler.cpp`, `rlvdefines.h`): the carrier is ordinary
**owner-say chat** (`CHAT_TYPE_OWNER` on channel 0) from an object the agent
owns — nothing new on the wire, which is why it works on any grid. A message
qualifies when it starts with `@` (`RLV_CMD_PREFIX`); the viewer swallows it so
it never appears in the chat log.

The payload is a **comma-separated list** of commands, each
`behaviour[:option]=param`, lower-cased. The `param` chooses the kind
(`ERlvParamType`):

- `n` / `add` adds a restriction, `y` / `rem` lifts it;
- `force` performs an action (`@sit:<uuid>=force`, `@remoutfit=force`);
- a **number** makes it a query whose answer is chatted back on that channel
  (`@version=2222`, `@getoutfit=1234`);
- plus `@clear` (with an optional filter).

Shape of the work: a **pure crate** (`sl-rlv`, no I/O, in the spirit of
`sl-prim` / `sl-anim`) whose parser decodes a chat line into a typed command
stream — behaviour, optional option, param kind, channel — covering the ~150
behaviours (`ERlvBehaviour`) and the version handshake tokens
(`@version` / `@versionnew` / `@versionnum`). This is grid-agnostic and
unit-testable to the letter: the reference's own `rlvfloaters.cpp` debug console
feeds hand-typed commands through the same path, which is exactly how this crate
should be tested. It must be usable *without* the Bevy viewer — a headless
client wanting to be RLV-compliant needs exactly this.

Reference (Firestorm, read-only): `rlvhandler.cpp`, `rlvdefines.h`
(`ERlvBehaviour`, `ERlvParamType`, `RLV_CMD_PREFIX`); command dispatch is
seeded from `llviewermessage.cpp` ~3141.
