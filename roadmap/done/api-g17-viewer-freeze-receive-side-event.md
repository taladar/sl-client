---
id: api-g17
title: Viewer freeze (receive-side event)
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G17 — Viewer freeze (receive-side event)

`ViewerFrozenMessage` (Low 137, `Trusted`, sim→viewer): a single
`FrozenData.Data` `BOOL` telling the viewer it has been frozen (`true`) or
thawed (`false`) by an estate manager (`llfreezeavatar` / the estate-tools
freeze). The frozen viewer suppresses its own movement/controls until thawed.
Receive-only — decode into an `Event`; the server side gets a matching
`SimSession::send_*` encoder. This was the one message deferred out of G13's
alert set (it is a freeze toggle, not an alert string), so it follows the same
receive-only pattern. OpenSim-testable (estate freeze).

- [x] G17 viewer freeze/thaw event. One receive-only `Event` decoded in
  the client dispatch (`session/methods.rs`): `ViewerFrozen { frozen }`
  (`ViewerFrozenMessage`'s single `FrozenData.Data` `BOOL` — `true` frozen,
  `false` thawed). Server: `SimSession` gains the matching encoder
  `send_viewer_frozen`. Both runtimes forward the `Event` generically (the
  tokio/bevy/survey login examples gained the new variant in their ignore
  arms) + REPL (`viewer_frozen` event name in `format.rs`). Tests: 1 lifecycle
  client (frozen→thawed toggle surfaces the event) + the G13 loopback
  round-trip extended to cover `send_viewer_frozen` (renamed
  `alerts_collisions_health_camera_frozen_reach_client`). Book:
  `content/world.md` "Simulator notifications" gained the viewer-freeze
  bullet plus an "In this codebase" entry. OpenSim-testable (estate freeze)
  but NOT live-tested this session (loopback + lifecycle cover both directions).
  **Scope note:** receive-only — no
  client command and no `ServerEvent` (the viewer never sends it; it is a
  `Trusted` sim→viewer message). This completes the Tier G tier list; remaining
  open items are the deferred follow-ups DF1/DF2 (gated on unrelated machinery).
