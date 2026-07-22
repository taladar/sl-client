---
id: viewer-voice-call-dialogs
title: Voice call dialogs — incoming / outgoing, channel switching
topic: viewer
status: blocked
origin: voice brought fully in scope for the viewer (user decision,
  2026-07-22)
blocked_by: [viewer-voice-audio]
refs: [chat-b8, viewer-voice-controls]
---

Context: [context/viewer.md](../context/viewer.md).

The call-lifecycle UI: **incoming call** (P2P call or ad-hoc/group voice
invitation — the signalling arrives via the per-session voice-channel
state [[chat-b8]] already models) with accept / decline and the
reference's "declining returns you to nearby voice" semantics; **outgoing
call** ringing state with cancel; and **channel switching** — one active
voice channel at a time, joining a group call leaves nearby voice and the
UI makes the current channel + a "leave call" action always evident
(the voice floater [[viewer-voice-controls]] shows it; the conversation
tabs get call-state badges and call/hang-up buttons).

Reference (Firestorm, read-only): `llfloaterincomingcall` /
`floater_incoming_call.xml`, `floater_outgoing_call.xml`,
`llvoicechannel` (channel state machine).

Deps: [[viewer-voice-audio]] (an actual call to connect; signalling
alone was already modelled in [[chat-b8]]).
