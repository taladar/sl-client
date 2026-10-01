---
id: viewer-rlv-receive-side-consumers
title: RLV — make the viewer's receive paths ask the RlvActions façade
topic: viewer
status: ready
origin: test-e2e-sweep-rlv (2026-10-01) — the Strings window's IM text reaches no IM
refs: [viewer-rlv-enforce-receive-side, viewer-rlv-send-side-consumers, viewer-rlva-floaters-toggles]
---

Context: [context/viewer.md](../context/viewer.md).

The receive-side twin of [[viewer-rlv-send-side-consumers]].
[[viewer-rlv-enforce-receive-side]] gave `sl_rlv::RlvActions` its incoming
half (`actions/receive.rs`), and its own "Verified" section ends on the same
gap the send side had: *still no consumer in the workspace*. On 2026-10-01
that is still true. Outside `sl-rlv`, nothing calls `incoming_chat`,
`incoming_im`, `can_receive_im`, `incoming_session_invite`,
`script_permission`, `away_timeout_seconds` or
`clears_away_on_animation_stop`.

So a worn object's `@recvchat`, `@recvemote`, `@recvim`, `@recvimfrom`,
`@accepttp`, `@accepttprequest`, `@acceptpermission` and `@allowidle` are
parsed, held and listed in the Restrictions window, and change nothing the
user sees. It is also why the RLVa Strings window's two `blocked_recvim`
strings — editable, kept and restorable, as `tests/e2e_rlv.rs` checks — are
never shown anywhere: no incoming IM is ever censored.

What this needs:

- The `RlvActionSource` the send side builds, over the viewer's world, shared
  by both halves (whichever lands first builds it).
- Each receive path asks the façade before it shows anything: local chat and
  emotes (the Nearby transcript and the chat overlay), one-to-one IMs (the
  censored line, and the `blocked_recvim_remote` reply to the sender), group
  and conference invitations (declined or censored), teleport offers and
  requests (auto-accepted), script permission requests (refused, granted or
  asked), and the away timer.
- An end-to-end test per path on the fake grid: hold the restriction through
  the RLVa console (`tests/e2e_rlv.rs` shows how), deliver the message through
  `FakeAgent::with_sim`, and read the conversation, the notification or the
  outbound reply — including that an edited `blocked_recvim` string is the one
  shown.
