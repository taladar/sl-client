---
id: chat-b5
title: Invitation handling + session lifecycle
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

## B5. Invitation handling + session lifecycle — DONE 2026-06-27

*(was old B5 + the lifecycle half of old B4 — from A4·A5.)* The lifecycle enum
is born **here**, with its only constructor (the invite path) and its reader, so
the `Invited` variant is never a dead, never-constructed variant. See § Session-
lifecycle reference (from A4) and § Invitation-handling reference (from A5).

- [x] Add `enum ChatSessionLifecycle { Invited(PendingInvite), Joined }` +
  `PendingInvite { inviter: AgentKey, session_name: String, channel:
  InviteChannel }` + `enum InviteChannel { Text, Voice, Both }`; add the
  `lifecycle` field to `ChatSession` (`ChatSession::new` default `Joined`, still
  `const`).
- [x] Promotion rule: any session message / participant traffic (every
  `chat_session_mut` lazy-open) sets `lifecycle = Joined`; the new
  `mark_chat_session_invited` is the sole `Invited` setter and never demotes a
  joined session. Typing (`chat_session_get_mut`) does not promote.
- [x] Classify the invite channel: `chatterbox_invitation_from_llsd`
  (`conversions.rs`) now also parses the top-level `voice`/`immediate` invite
  shape; the sibling `invite_channel_from_llsd` reads the `instantmessage` /
  `voice` sub-maps. The `ChatterBoxInvitation` handler get-or-creates the
  `Group`/`Conference` entry as `Invited(PendingInvite { .. })`, still emitting
  `Event::ConferenceInvited` unchanged.
  - **Deviation:** the planned **UDP `SessionGroupStart` /
    `SessionConferenceStart` inbound dispatch** was *not* added — verified
    against OpenSim (`GroupsMessagingModule.cs:648`) and the reference viewer
    (`llimview.cpp:5047`) that those dialogs are **outbound** (client→sim) and
    that invites are delivered **only** via the `ChatterBoxInvitation` CAPS
    event, so inbound UDP arms would be dead code (which the consolidation
    forbids).
- [x] Add `Command::AcceptChatInvite` /
      `DeclineChatInvite { session_id, from_group }` +
      `Session::accept_chat_invite` / `decline_chat_invite` (accept → `Joined`;
      decline → remove). The CAPS accept-reply roster decodes into the B3
      participant list (`chat_session_roster_from_llsd`, both `agent_info` map
      and `agents` array).
- [x] Runtime dual path (tokio / bevy / REPL at parity): look up
  `CAP_CHAT_SESSION_REQUEST`; if present POST `{ method, session-id }`
  (`chat_session_request_body`) with the fixed method (`"accept invitation"` /
  `"decline invitation"` — both text and multi-agent voice; the voice-join
  signalling + P2P `"decline p2p voice"` stay deferred to B8), via the new
  `post_chat_session_request` / `run_chat_session_request` helper that tags the
  reply roster with its session id; if absent, decline sends `SessionLeave`
  (`leave_group_session` / `leave_conference`). Cap added to the requested-caps
  set.
- [x] Surface `lifecycle` through the new `chat_session_lifecycle()` accessor
  (so the field is read before B7).
- [x] Tests: text `ConferenceInvited` (group + conf) → `Invited(channel=Text)`;
      voice → `Voice`/`Both`; `accept_chat_invite` → `Joined`; inbound traffic
      also promotes; `decline_chat_invite` removes; per-channel CAPS
      `method`/`session-id` LLSD unit; accept-reply roster decodes to
      participants.

  **Done — see § Invitation-handling reference (from A5) + task B5. The new
  sans-IO tests live in `lifecycle.rs`:
  `chatterbox_invitation_records_pending_invite`,
  `chatterbox_invitation_classifies_voice_channels`,
  `accept_and_decline_chat_invite_transition`,
  `inbound_traffic_promotes_invited_to_joined`,
  `chat_session_request_roster_seeds_participants`,
  `chat_session_request_body_encodes_method_and_session`.**
