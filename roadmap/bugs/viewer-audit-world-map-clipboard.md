---
id: viewer-audit-world-map-clipboard
title: The world map keeps a second live arboard handle
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 1
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-map/src/world_map.rs:195-198` and `:2785-2807` define
`WorldMapClipboard(Mutex<Option<arboard::Clipboard>>)` plus a byte-identical
`copy_to_clipboard`, duplicating
`sl_viewer_platform::clipboard::{ViewerClipboard, copy_to_clipboard}` — whose
module doc says *"The world map keeps its own handle for historical reasons; new
'Copy' sites share this one."*

`sl-viewer-map` **already depends on `sl-viewer-platform`**, and five other
sites use the shared one (`about_landmark.rs:41`, `avatar_profile.rs:2501`,
`group_profile.rs:2676`, `debug_settings.rs:47`, `about_floater.rs:26`).

Two live `arboard` handles is a real hazard on Wayland, where the selection
owner is the process holding the connection.

Fix: delete both, take `Res<ViewerClipboard>`, and drop `arboard` from
`sl-viewer-map/Cargo.toml`.
