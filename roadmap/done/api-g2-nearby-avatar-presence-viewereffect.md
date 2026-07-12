---
id: api-g2
title: Nearby-avatar presence & ViewerEffect
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G2 — Nearby-avatar presence & ViewerEffect

`CoarseLocationUpdate` (Event: nearby avatar minimap positions/heights);
`ViewerEffect` (send + receive: look-at, point-at, beam, sphere, selection
highlight — new `ViewerEffectType` enum); `TrackAgent` / `FindAgent` +
`FindAgentReply`. OpenSim-testable.

- [x] G2 coarse-location, viewer effects, agent tracking. New
  `sl-proto/src/types/nearby.rs`: `CoarseLocation`; `ViewerEffect` +
  `ViewerEffectType` (the `LLHUDObject` effect codes) + `ViewerEffectData`
  (typed `LookAt`/`PointAt` 57-byte + `Spiral`-family 56-byte `TypeData`, with a
  `Raw` fallback) + `LookAtType`/`PointAtType`. Commands `ViewerEffect`
  (batched), `TrackAgent`, `FindAgent`; events `CoarseLocationUpdate`,
  `ViewerEffect`, `FindAgentReply`. Server decodes the inbound client messages
  into matching `ServerEvent`s and gains `send_coarse_location_update` /
  `send_viewer_effect` / `send_find_agent_reply` encoders. Both runtimes + REPL
  (`viewer_effect`/`track_agent`/`find_agent`) + tests (5 client + 5 server) +
  new book chapter `content/nearby.md`. **Scope note:** there is no separate
  `FindAgentReply` message — `FindAgent` is reused for both the request and the
  filled-in reply (modelled as `Command::FindAgent` + `Event::FindAgentReply`),
  exactly as the viewer/sim use it.
