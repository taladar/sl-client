---
id: protocol-sim-caps-agent-comms
title: Server-side agent-communication caps
topic: protocol
status: done
origin: user request (2026-07) — complete simulator protocol surface
points: 5
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/protocol.md](../context/protocol.md).

The agent-communication cluster, server side:

- `ChatSessionRequest` — the group-chat ChatterBox session lifecycle,
  including the `ChatterBoxInvitation` /
  `ChatterBoxSessionAgentListUpdates` event-queue events;
- `ReadOfflineMsgs`;
- `GetDisplayNames` (paired with the existing `DisplayNameUpdate` EQ
  helper);
- `AgentPreferences`;
- `SendUserReport` / `SendUserReportWithScreenshot`.

Inverse-pairing per the convention; verified against the client-direction
builders/parsers in-memory.

Done (2026-08-14): six `CapHandler` variants routed in `SimCaps`
(`sl-proto/src/sim_caps.rs`) and their coverage-table rows flipped to
Served. New inverses: `chat_session_request_from_llsd`,
`chat_session_roster_to_llsd`, `session_history_to_llsd`, and
`agent_list_voice_updates_to_llsd` (+ the
`enqueue_chatterbox_agent_list_updates` EQ helper) beside their client
pairs in `conversions.rs`, and `build_asset_upload_response` in
`sl-wire/src/llsd.rs` for the two-step screenshot uploader (uploader URL
= the cap's own `screenshot` sub-path). `SimSession` grew the serving
state: a deliver-once offline-message store, a display-name store,
merge-and-echo agent preferences at OpenSim defaults (`god_level`
reply-only), chat-session accept/decline + `record_session_history`,
and `ServerEvent::AbuseReportWithScreenshotReceived`. Loopback coverage
in `sl-proto/tests/sim_caps.rs` drives the real client `Session` folds
(roster, history, invitation + agent-list EQ push, offline IMs); book
coverage in the "The agent-communication handlers" subsection of
`book/src/comms/caps.md`.
