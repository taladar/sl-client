---
id: protocol-sim-udp-flows-2
title: Server-side mirrors for the remaining Session flow machines
topic: protocol
status: ready
origin: follow-up from the protocol-sim-udp-flows flow audit (2026-08)
points: 5
refs: [protocol-sim-udp-flows, protocol-sim-caps-agent-comms]
---

Context: [context/protocol.md](../context/protocol.md).

The pinned flow-coverage table (`SESSION_FLOW_COVERAGE` +
`flow_coverage_table_is_pinned`, from [[protocol-sim-udp-flows]]) left
four client `Session` flow machines `Pending`. Mirror them in
`SimSession`, each proven by `Session` ↔ `SimSession` loopback tests,
and flip their rows to `Mirrored`:

- **Object sit**: `AgentRequestSit` → `AvatarSitResponse` (sit
  transform, camera offsets, autopilot flag) → `AgentSit`, plus the
  stand-up via the control flags — the mirror of the client's
  `SitState` machine.
- **Script permission / control mirror**: send `ScriptQuestion`,
  receive `ScriptAnswerYes`, drive `ScriptControlChange` — the mirror
  of the client's `ScriptGrant`/`TakenControls` tracking.
- **Friendship / presence**: relay the IM-dialog offer/accept/decline
  handshake, `OnlineNotification`/`OfflineNotification` presence
  pushes, `ChangeUserRights`, and `TerminateFriendship` — the mirror of
  the client's friends/online registries (2-avatar shape: one
  SimSession per client, driver relays).
- **Chat-session lifecycle + server history**: group/conference session
  membership + message relay over `ImprovedInstantMessage` session
  dialogs and `ChatterBoxInvitation` — the mirror of the client's
  `ChatSession` lifecycle. The CAPS half (`ChatSessionRequest`, the
  "fetch history" tag) overlaps [[protocol-sim-caps-agent-comms]] —
  keep the session *state* here and the cap dispatch there.

Stateless request/reply surfaces (money, selection, appearance, group
management edits, directory queries) stay non-rows: `SimSession`'s
canned `send_*` replies already cover them without a machine.
