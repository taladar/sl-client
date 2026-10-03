---
id: protocol-agent-list-voice-transition-lossy
title: Agent-list voice updates collapse ENTER/LEAVE and can-voice into one bool
topic: protocol
status: bugs
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
