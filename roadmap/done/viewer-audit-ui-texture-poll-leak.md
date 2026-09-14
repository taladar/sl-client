---
id: viewer-audit-ui-texture-poll-leak
title: Eight copied texture-poll systems each leak Image assets for the session
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 5
refs: [viewer-audit-decoded-texture-uploaders]
---

Context: [context/viewer.md](../context/viewer.md).

The "poll a decoded texture into an `ImageNode`" system is written eight times —
`sl-viewer-people/src/group_profile.rs:2947`, `avatar_profile.rs:2872`,
`group_notice.rs:685`, `sl-viewer-inventory/src/inventory_gallery.rs:674`,
`inventory_properties.rs:1094`,
`sl-viewer-pickers/src/ui_texture_picker.rs:1163`,
`sl-viewer-search/src/search.rs:3097` (which labels itself "the
`poll_profile_textures` pattern") and
`sl-viewer-places/src/about_landmark.rs:691`.

Every copy does a bare `images.add(to_bevy_image(decoded))` with **no
`TextureKey -> Handle<Image>` dedup and no removal**, so re-showing the same
thumbnail allocates another full RGBA image that lives until exit.

The world layer already solved this five times over —
`sl-viewer-world-objects/src/textures.rs:882`, `legacy_materials.rs:98` and
`:104`, `sl-viewer-world-scene/src/terrain.rs:160`,
`sl-viewer-world-avatar/src/avatars.rs:2430` all keep a
`HashMap<TextureKey, Handle<Image>>`.

Scope: a `UiTextureImages` resource in `sl-viewer-world-api` beside
`DecodedTextures`, holding that map plus one `poll_ui_textures` system driven by
a `PendingUiTexture { key, node }` component. The eight systems collapse to
inserting that component, and the leak is fixed once.

Note `DecodedTextures` itself (`sl-viewer-world-api/src/lib.rs:6559`) has no
eviction path either, and these seven crates feed it from about 40 request
sites.

## What landed

`sl-viewer-world-api/src/ui_texture.rs`: one `UiTextureImages` resource, one
`PendingUiTexture` component and one `poll_ui_textures` system, wired by a
`UiTexturePlugin` each of the six floater crates adds behind an
`is_plugin_added` guard (none of them owns the others, and a test app that
schedules only one must still paint its images). The eight copies are gone; a
surface that wants a texture inserts the component on the box and nothing else.

Three things the map does that the scope did not spell out:

- The **subject is the node**, not a list on the window. A rebuilt or closed
  window takes its pending textures with it, so a decode that lands afterwards
  has nothing to paint — each per-window list had to check that for itself, and
  the picker's shared `waiting` map could still let the *first* of two quick
  selections land on the pane after the second. The picker's preview pane and
  its swatches now hold one pending each, newest wins, and a swatch cleared to
  the nil texture drops its wait along with its image.
- The upload is keyed by texture id **and** the decoded level of detail, so a
  texture that re-decodes finer is uploaded again rather than every later window
  being served the coarse image for the rest of the session.
- The map holds the upload **weakly** (an `AssetId`, not a `Handle`), so what
  keeps an image alive is the nodes showing it. When the last window showing a
  thumbnail closes Bevy drops the image, and the next window that wants it
  uploads it again — the footprint is bounded by what is on screen rather than
  by everything the session has ever displayed, which is the "and no removal"
  half of the finding.

Asking for the texture moved into the shared piece too (`Changed`, not `Added`,
so re-pointing a box at a second texture asks for that one as well): the eight
`BoostTexture` writers went with the eight polls, and a box that waits is a box
that asked. It arrives a frame later than the spawn that used to write it, which
is why `reopening_a_texture_does_not_refetch_it` now drains the message stream
rather than reading one frame of it.

Six tests in `ui_texture.rs` pin the map (one upload for two nodes, a finer
decode re-uploaded, a released upload rebuilt, the placeholder dropped only
where asked, the ask-then-stop-waiting cycle, and a re-pointed box). The
texture-preview window's own `TexturePreviewState` went with its poll — it held
nothing else — so its tests identify a window by the `FloaterKey` it is keyed
by, which is what a preview window actually is.

`DecodedTextures` still has no eviction path; that half of the finding is
untouched and stays where it is filed.
