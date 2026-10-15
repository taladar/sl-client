---
id: server-fake-grid-group-chat
title: Fake grid — group chat sessions
topic: server
status: blocked
origin: server-fake-grid-groups (2026-10-15)
refs: [server-fake-grid-groups]
blocked_by: [gridspec-group-chat-and-conference]
---

Context: [context/server.md](../context/server.md).

## What

The fake grid holds groups ([[server-fake-grid-groups]]) but no group chat:
a `SessionGroupStart` and the messages sent into a group session go nowhere.
Fan group session messages out to the members as each grid does, with the
history Second Life keeps and the membership propagation delay it shows.

## How

Implement what [[gridspec-group-chat-and-conference]] measures, per
`ImitatedGrid` flavour (a field of `GroupsPolicy` per divergence), and hold
it to the measurement with that task's conformance cases run against `FakeSl`
and `FakeOpensim`.
