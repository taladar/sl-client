---
id: viewer-avatar-debug-tools
title: Avatar debug & maintenance tools (incl. Rebake)
topic: viewer
status: ready
origin: Advanced/Develop menu survey (2026-07-22)
refs: [viewer-render-metadata-overlays]
---

Context: [context/viewer.md](../context/viewer.md).

The Develop → Avatar utility set. Two of these are everyday **user**
features, not debug: **Rebake Textures** (Ctrl+Alt+R — rerun the appearance
/ bake pipeline and republish, the universal "I am a cloud" fix) and
**Refresh Attachments** (re-request attachment objects). Both get menu
entries + keybind and must work reliably; the bake pipeline
(`appearance.rs`, `bake_publish.rs`) already has the pieces.

The rest is a debug checklist, implemented as cheaply as the data allows:

- [ ] Appearance-to-XML dump (worn wearables + visual params to a file —
      pairs with the reference format for diffing)
- [ ] Grab baked texture to disk (save a bake channel as PNG)
- [ ] Animation speed 10% faster/slower/reset + slow motion
- [ ] Force visual params to default
- [ ] Animation info overlay (playing anims + priorities over each avatar)
- [ ] Debug avatar textures floater (per-channel bake/texture state)
- [ ] Test male / test female canned shapes

Reference (Firestorm, read-only): `menu_viewer.xml` (Develop → Avatar),
`llvoavatarself` (rebake), `llfloateravatartextures`.

Builds on: the avatar appearance/bake pipeline and the animation registry.

## Parity-audit addendum (2026-08-19)

The parity audit adds four Develop ▸ Avatar entries not yet in the
body: **Toggle Character Geometry**, **Allow Select Avatar** (permit
selecting avatars with the build tool), avatar **Disable LOD**, and
the **Dump Local Textures** dump. (Debug Joint Updates / Character
Vis / Rez
State / Debug Rotation are implemented differently via our
SL_VIEWER_LOG_AVATAR_* env toggles and the avatar dump harness; Dump
Attachments exists as `avatar_dump.rs`.)
