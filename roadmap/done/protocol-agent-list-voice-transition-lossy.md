---
id: protocol-agent-list-voice-transition-lossy
title: Agent-list voice updates collapse ENTER/LEAVE and can-voice into one bool
topic: protocol
status: done
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

`sl-proto/src/session/conversions.rs` (~4517-4570) folds a chat-session
agent-list update's `transition` (`"ENTER"` / `"LEAVE"`) and
`can_voice_chat` into one "in voice now" bool, and the server-side encoder
turns `false` back into `"LEAVE"`.

## Effect

A member who is present but text-only is announced as leaving by our server
side (fake grid, `SimSession`), and the client cannot tell "left" from "has
no voice".

## Fix

Carry `(Transition, can_voice)` separately (`enum AgentListTransition { Enter,
Leave }`), encode them as the reference does. Round-trip test.

## Fixed (2026-10-15, gridspec-group-chat-and-conference)

`ChatSessionAgentUpdate` carries what one entry says — the `SessionTransition`
(`Enter` / `Leave` / `Unrecognised`), `can_voice_chat`, `is_moderator`, the
text and voice mutes, and whether it came from the old `updates` map — each
optional. `agent_list_updates_from_llsd` reads it as the reference does,
`agent_list_updates_to_llsd` (renamed from `agent_list_voice_updates_to_llsd`)
writes it back leaving out what an update does not state, and
`SimSession::enqueue_chatterbox_agent_list_updates` takes the entries. The
client folds the transition into the roster and the voice flag into the voice
members separately. Round-trip test with a member who stays without voice.
