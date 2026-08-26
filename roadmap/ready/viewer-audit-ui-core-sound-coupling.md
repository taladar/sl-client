---
id: viewer-audit-ui-core-sound-coupling
title: Three lines in ui_sounds.rs put the protocol stack behind 22 crates
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-ui-core/src/ui_sounds.rs:51-53` — `use sl_audio::{...}` and
`use sl_client_bevy::{AssetKey, Uuid}` — are the **only** reason
`sl-viewer-ui-core/Cargo.toml` depends on `sl-audio` (2.5k lines) and
`sl-client-bevy` (13.9k lines, which itself pulls `sl-proto`, `sl-wire`,
`sl-asset`, `sl-bake`, `sl-mesh`, `reqwest`, `tokio`, `wgpu-types`).

**22 crates depend on `sl-viewer-ui-core`**, most of them only for
`ui::column()` and `ui_font`. And `AssetKey` originates in
`sl-proto/src/asset_keys.rs:40` — a wire type reaching the crate whose doc says
"Nothing here knows what a floater or a tab is".

`ui_sounds` has three external consumers. Lift it to a sibling crate over
ui-core, exactly as `sl-viewer-ui-pie-menu` was made one. This is the biggest
single build-graph win available in the UI cluster.

Trivial companion in the same crate: `ui_pseudoloc` is `pub mod` with **zero**
external consumers (only `i18n.rs` uses it) — `pub(crate)` would say so.
