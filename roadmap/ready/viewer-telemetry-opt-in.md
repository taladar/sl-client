---
id: viewer-telemetry-opt-in
title: Opt-in viewer telemetry, saying exactly what is sent
topic: viewer
status: ready
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, protocol-avatar-render-info]
---

Context: [context/viewer.md](../context/viewer.md).

Second Life grants four reporting capabilities; Firestorm sends three of them
with no way to turn them off. Ours sends nothing unless the resident opts in, on
a preferences page that lists every field each report carries:

- **`ViewerStats`** — POST every 300 s and at quit: run time, frame rates,
  simulator frame rate, avatars in view, ping, distance travelled, regions
  visited, memory use, the operating system, CPU, RAM, GPU and driver,
  download totals by kind, network totals and failure counters (Firestorm's
  `send_viewer_stats`). The UDP form is deprecated.
- **`ViewerMetrics`** — two POSTs: asset-fetch metrics every 600 s
  (per-region counts and response times by asset type and transport) and
  appearance metrics every 300 s (rez status, decloud times, nearby avatar
  counts).
- **`TextureStats`** — texture and object cache statistics; Firestorm sends it
  only with a debug setting.
- **`SearchStatTracking`** — POST `{type, from_search, classified_id,
  parcel_id, dest_pos_global, region_name}` when a classified is opened, mapped
  or teleported to: click tracking for advertisers.

Default off; each report its own switch; the page shows the field list and,
where possible, the last payload that was sent.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.
