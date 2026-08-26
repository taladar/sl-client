---
id: viewer-audit-preferences-hub-decoupling
title: sl-viewer-preferences is a 12-crate hub whose own decoupling mechanism is under-applied
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-preferences/src/lib.rs:31-81` carries 50 `pub(crate) use` re-aliases
across 12 crates: `sl-viewer-audio` (4 modules), `sl-viewer-kit` (2),
`sl-viewer-notifications`, `sl-viewer-platform` (2), `sl-viewer-settings` (+
`keys`), `sl-viewer-ui-core` (7), `sl-viewer-ui-widgets` (8),
`sl-viewer-world-api`, `sl-viewer-world-avatar` (3), `sl-viewer-world-objects`
(3), `sl-viewer-world-scene` (8), `sl-viewer-world-view` (4).

**The registry that decouples it already exists**:
`sl-viewer-settings/src/keys.rs` was created for exactly this and already
removed the `sl-viewer-people` and `sl-viewer-map` edges (41 constants).
Enumerating every symbol preferences names shows three more edges are removable
the same way:

- `sl-viewer-world-objects` — `hover_text::SETTING_SHOW_HOVER_TEXT`,
  `name_tag_billboard::SETTING_*` (5),
  `render_priority::{SETTING_LOD_FACTOR, LOD_FACTOR_MIN, LOD_FACTOR_MAX, register_settings}`.
  **Nothing but constants**, so the whole crate dependency goes;
- `sl-viewer-world-avatar` — `derender::SETTING_FRIENDS_ONLY`,
  `name_tag_content::SETTING_*` (10), `avatar_complexity::{SETTING_*,
  *_SLIDER_MAX/STEP, ComplexityMode}`. Constants plus one enum;
- `sl-viewer-world-scene` — `glow`, `exposure`, `tonemap`, `probes`,
  `particles`, `parcel_borders` contribute **only** `SETTING_*` / `TONEMAP_*`
  constants and `register_settings`; only `sky::{SceneSun, shadow_cascades_for,
  sun_shadows_enabled}` and `environment::*` are real behaviour. Six of eight
  module aliases are constants.

**Not a finding, for the record**: the preferences panels themselves are the
best-factored area in the viewer. `preferences.rs:356-600` defines seven shared
row builders and all eight tabs use them consistently (alerts 15 calls, audio
17, camera_move 19, chat 41, colors_skins 10, general 31, graphics 38,
network_cache 17), and save/revert is one shared snapshot lifecycle at
`:790-905`.

The real duplication is `quick_preferences.rs` (1724 lines), which re-renders
six settings that already have tab rows (`SETTING_RENDER_QUALITY`,
`SETTING_DRAW_DISTANCE`, `SETTING_LOD_FACTOR`, `SETTING_MAX_COMPLEXITY`,
`SETTING_MAX_PARTICLES`, `derender::SETTING_FRIENDS_ONLY`) using the lower-level
`bound_checkbox` / `bound_slider` / `spawn_combo` instead of the `spawn_pref_*`
layer — a second, parallel row stack.
