---
id: chat-b8
title: Per-session voice-channel state (signalling only)
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

## B8. Per-session voice-channel state (signalling only) — DONE 2026-06-27

*(was old B12 — from A12.)* After B5 (the `ChatSessionRequest` cap + invite
classification) and B7 (`ChatSessionInfo`). Reuses the existing voice-signalling
feature; **no** audio transport / speaking indicators. See § Per-session voice-
channel reference (from A12).

- [x] Add `VoiceChannelState { has_voice, channel: Option<VoiceChannelInfo>,
  joined, members: BTreeSet<AgentKey> }` + `VoiceChannelInfo { channel_uri,
  channel_credentials, voice_server_type, session_handle }` (client-local,
  `Default`-able) and a `voice: VoiceChannelState` field on `ChatSession`
  (empty / no-voice in `ChatSession::new`, which stays `const`).
- [x] Decode the gaps: the `ChatterBoxInvitation` handler reads the `voice` body
      into `has_voice` / `channel` via the new `voice_channel_info_from_llsd`
      (the decode lives in the handler beside `mark_chat_session_invited`, not
      inside `chatterbox_invitation_from_llsd` itself — that returns an `Event`
      with no voice slot; same split as B5's `invite_channel_from_llsd`); decode
      the `ChatSessionRequest "accept invitation"` reply `voice_channel_info`;
      decode the `ChatterBoxSessionAgentListUpdates` agent-list voice flag
      (`info.can_voice_chat`, **not** `is_now_speaking`) into `members`.
- [x] Add `Command::JoinSessionVoice` / `LeaveSessionVoice { session }` +
  `Session::join_session_voice` / `leave_session_voice` (optimistic
  `voice.joined`). Runtime orchestration at parity (tokio/bevy/REPL): join =
  provision a voice account (vivox) then the `ChatSessionRequest` accept;
  leave = `"decline invitation"` (multi-agent) / `"decline p2p voice"`
  (1:1 `Direct`) —
  reusing `post_voice_cap` / `post_chat_session_request` (B5). New
  `CHAT_SESSION_DECLINE_P2P_VOICE` constant.
- [x] Accessors `session_has_voice` / `session_voice_channel` /
  `session_voice_joined` / `session_voice_members`; voice fields (`has_voice`,
  `voice_joined`, `voice_members`) added to `ChatSessionInfo` (B7) and populated
  in `chat_sessions_info()`.
- [x] Fold `voice.members` into B6's offline fan-out; voice state persists a
  teleport (verified by test), clears on logout (rides `ChatSession`).
- [x] Tests (7 in `lifecycle.rs`): voice invitation sets `has_voice` and
  `channel`; accept-reply populates `VoiceChannelInfo`;
  `Join`/`LeaveSessionVoice` flip `session_voice_joined`; an agent-list voice
  update folds `members` (speaking flag ignored, `LEAVE` removes); 1:1 P2P voice
  carries `{ self, peer }`; `OfflineNotification` drops a friend from
  `voice.members`; a teleport preserves the voice facet.
