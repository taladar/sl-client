---
id: viewer-key-texture-preview
title: One texture-preview window per texture
topic: viewer
status: ready
origin: split out of [[viewer-keyed-floater-audit]] (2026-09-07)
points: 2
refs: [viewer-keyed-floater-audit, viewer-key-animation-preview,
  viewer-profile-floater-single-instance]
---

Context: [context/viewer.md](../context/viewer.md).

`inventory_properties.rs`'s **texture preview** (`"preview-texture"`) is still
a singleton: opening a second texture repoints the one window, so the first
texture is gone. Comparing two textures side by side is an ordinary workflow —
which is exactly the argument that converted the asset editors — and this
window was missing from the audit list rather than deliberately excluded.

Convert it onto the keyed scaffold ([[viewer-profile-floater-single-instance]]),
following the worked examples (`avatar_profile.rs`, `edit_notecard.rs`,
`about_landmark.rs`):

- **Key** by the texture's asset id (a `FloaterKey::subject`, so nothing is
  persisted per instance).
- Move the window's slice of `PreviewState` — `pending_texture`, the image node
  it fills — onto the window root as a component. `PreviewState` currently also
  holds the animation preview's field; [[viewer-key-animation-preview]] takes
  that half, and whichever lands second deletes the resource.
- Open through `KeyedFloaters::open`, build the content at spawn instead of
  through `DeferredFloaterContent`, and order the open system
  `.after(FloaterSystems::Commands)` — an Open from an inventory row also
  raises the inventory window, and the later raise wins.
- Re-opening a texture already up **raises** it; it must not re-fetch.

## How to verify

Open two different textures from inventory: two windows, each showing its own
texture, each closable without disturbing the other; re-opening one raises it.
A unit test in the module's `instances` block, mirroring
`two_landmarks_open_two_windows`, pins it.
