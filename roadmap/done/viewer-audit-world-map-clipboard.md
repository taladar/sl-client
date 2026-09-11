---
id: viewer-audit-world-map-clipboard
title: The world map keeps a second live arboard handle
topic: viewer
status: done
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

## Resolved (2026-09-11)

Done as written: `WorldMapClipboard` and the map's copy of `copy_to_clipboard`
are gone, `handle_world_map_actions` takes `Res<ViewerClipboard>`, and
`sl-viewer-map` no longer depends on `arboard` at all. There is now exactly one
`arboard` handle in the process, which is the whole point on Wayland — a second
one is a second selection owner, and which of the two the compositor asks for
the bytes is not the viewer's to decide.

One thing the fix note did not say: the resource now comes from *another
crate's* plugin. `ClipboardPlugin` is added by the viewer binary, and today it
is added before `WorldMapPlugin`, so the live app is fine — but a missing
resource is not a loud failure in Bevy. It skips the system for an unresolved
parameter, and that system is the world map's *whole* action dispatcher: Copy
SLURL, Center on Me, teleport, every zoom level and every layer toggle would go
quiet together. So `WorldMapPlugin` also `init_resource`s it — idempotent, so it
neither conflicts with `ClipboardPlugin` nor opens a second handle — and
`the_plugin_registers_the_shared_clipboard` pins that, standing the plugin up
alone and asserting the resource is there.

The shared module's doc no longer claims "the world map keeps its own handle for
historical reasons"; it says why there is one handle instead.
