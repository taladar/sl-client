---
id: viewer-audit-kit-single-consumer-split
title: sl-viewer-kit is a grab-bag: 39% of it has exactly one consumer
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 5
refs: [build-split-ui-widgets-crate]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-kit/src/lib.rs:4` opens "This is a deliberately mixed bag, and the
mix is the point."

Eight of its twenty modules have exactly **one** consumer crate:
`radar_model` (1027 lines, used by `sl-viewer-people`), `shadow_visibility`
(740, the binary), `edit_math` (650, `sl-viewer-edit`), `world_map_math` (631,
`sl-viewer-map`), `ik` (415, `sl-viewer-world-avatar`), `sit_offset` (305, the
binary), `appearance` (258, the binary), `procedural` (230,
`sl-viewer-world-avatar`).

That is **4256 of 10985 lines (39%)** meeting the same single-consumer test the
`sl-viewer-ui-widgets` refactor already applied — see
[[build-split-ui-widgets-crate]]. The "leaf position, not subject matter"
defence is real, but it does not explain why `radar_model` compiles for
`sl-viewer-world-scene`.

Zero-test files here worth naming while the modules move: `avatar_assets.rs`
(514), `face_material.rs` (375), `sky_presets.rs` (322), `probe_layers.rs`
(136).
