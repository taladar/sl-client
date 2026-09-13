---
id: viewer-key-texture-preview
title: One texture-preview window per texture
topic: viewer
status: done
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

## Done (2026-09-12)

Keyed by the **asset**, not the item: two inventory copies of one texture are
one picture, so they share a window
(`two_items_sharing_a_texture_share_a_window`). `TexturePreviewState` — the
texture and the placeholder node awaiting it — is a component on the window
root, and `poll_texture_preview` iterates the open windows, so two decodes in
flight each fill the placeholder their own window spawned
(`a_decode_fills_only_the_window_waiting_for_it`). The window's title is the
item's name: the spec's "Texture" names the kind, which stopped being enough
the moment there could be two.

Re-opening a texture already up only raises it and fetches nothing
(`reopening_a_texture_does_not_refetch_it`) — the second Open used to tear the
content down and put "(loading)" back over a decoded image.

Landing this together with [[viewer-key-animation-preview]] deleted the shared
`PreviewState` and `PreviewUi` resources and the `Startup` spawn with them: the
plugin now spawns nothing at startup, since all three of its windows open per
subject.
