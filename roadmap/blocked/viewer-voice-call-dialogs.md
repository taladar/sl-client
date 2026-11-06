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

## Parity-audit addendum (2026-08-19)

Parity-audit extension: the friends-only gate — only friends may start
voice calls / IM sessions (`VoiceCallsFriendsOnly`, privacy tab) — and
per-type call auto-reject toggles: reject group calls
(`VoiceCallsRejectGroup`), ad-hoc/conference calls, and P2P calls
(the `VoiceCallsReject*` family on the sound tab).

## Voice invitations are left alone until this lands (2026-11-05)

Since [[viewer-session-invitation-accepted-at-once]] the conversations panel
accepts a *text* chat-session invitation at once and **ignores a voice-only
one** (`Event::ConferenceInvited::channel == InviteChannel::Voice`, logged at
info), which is what the reference does while voice is not working. The
incoming-call prompt this task builds is where such an invitation should go.
Accepting it is `Command::AcceptChatInvite` plus the voice join, and declining
it is `DeclineChatInvite` (or `decline p2p voice` for a P2P call).
