---
id: api-g16
title: Reports, postcards & map tiles
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G16 — Reports, postcards & map tiles

`UserReport` (abuse report; SL has a CAPS variant `SendUserReport` — add both),
`SendPostcard`, `MapLayerRequest`/`MapLayerReply` (world-map image tiles,
complementing the existing `RequestMapBlocks`/`RequestMapItems`). Mixed.

- [x] G16 abuse reports, postcards, map-layer tiles. One new hand-written LLSD
      codec module in `sl-wire`: `abuse_report.rs` (the shared `AbuseReport` /
      `AbuseReportType` payload plus `build_send_user_report` →
      `parse_send_user_report` for the `SendUserReport` capability). The three
      UDP messages — `UserReport` (Low 133), `SendPostcard` (Low 412), and the
      `MapLayerRequest` (Low 405) / `MapLayerReply` (Low 406) pair — are already
      in the generated codec; `MapLayer` (in `types/map.rs`) and `Postcard` (new
      `types/report.rs`) are the decoded types. New cap `CAP_SEND_USER_REPORT`
      (added to `REQUESTED_CAPABILITIES`). Commands `RequestMapLayer`,
      `SendAbuseReport` (UDP), `SendAbuseReportViaCaps` (the capability POST),
      and `SendPostcard`; event `MapLayers`. Client `Session::request_map_layer`
      / `send_abuse_report` / `send_postcard` (circuit encoders
      `send_map_layer_request` / `send_user_report` / `send_postcard`), receive
      decode `MapLayerReply` → `Event::MapLayers`. Server
      `SimSession::send_map_layer_reply`, plus the receive-side decodes
      `UserReport` → `ServerEvent::AbuseReportReceived` and `SendPostcard` →
      `ServerEvent::PostcardReceived`. Both runtimes (the CAPS abuse report uses
      a new fire-and-forget POST helper `post_caps_oneway` / `run_caps_oneway`,
      since the cap returns only an HTTP status) + REPL (`request_map_layer`,
      `send_abuse_report`, `send_abuse_report_caps`, `send_postcard` registry
      commands) + format.rs event/command names. Tests: 2 wire round-trip
      (`abuse_report`) + 3 proto decode/encode (`map_layer_reply`, UDP
      `UserReport`, UDP `SendPostcard`) + 3 `SimSession` loopback (map-layer
      reply to client, abuse report & postcard to server) + 4 REPL registry.
      Book: extended `content/world.md` with "The world map", "Reporting abuse &
      filing postcards", and "In this codebase". **Scope note:** the map-layer
      pair mirrors the existing `RequestMapBlocks`/`RequestMapItems` exactly
      (client request + receive decode + `SimSession` reply encoder, no
      `ServerEvent` for the incoming request); `UserReport`/`SendPostcard` are
      client→sim fire-and-forget UDP messages, so they get `ServerEvent`s but no
      reply. The `SendUserReport` capability is the one CAPS member (HTTP,
      out-of-band): the server side is `parse_send_user_report` (no response
      body to build, so no `build_*_response`); the
      `SendUserReportWithScreenshot` snapshot-upload variant is out of scope
      (the plain no-screenshot cap covers the path). SL serves the cap and all
      the UDP messages; OpenSim implements only the UDP path (abuse report, map
      layers, postcard). NOT live-tested this session (wire + lifecycle +
      loopback round-trips cover both directions). **NEXT = G17** (the
      `ViewerFrozenMessage` freeze/thaw receive-side event).
