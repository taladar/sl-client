---
id: viewer-audit-ui-texture-poll-leak
title: Eight copied texture-poll systems each leak Image assets for the session
topic: viewer
status: bugs
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
