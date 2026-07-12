---
id: protocol-24
title: Media-on-a-prim / parcel audio (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**24. Media-on-a-prim / parcel audio (done) ✅ — CAPS
`ObjectMedia`/`ObjectMediaNavigate`, `ParcelMediaCommandMessage`,
`ParcelMediaUpdate` · 5 pts.** Per-face media on the scene (#16) plus the
parcel's streaming-media control surface. Two halves:

- **Object media-on-a-prim (CAPS, read + write).** A new `MediaEntry` wire value
  type (`sl-wire`) faithfully mirrors the viewer's `LLMediaEntry` — the eleven
  per-face fields (`current_url`/`home_url`, `auto_loop`/`auto_play`/
  `auto_scale`/`auto_zoom`, `first_click_interact`, `width_pixels`/
  `height_pixels`, `controls`, the white-list, and the `perms_interact`/
  `perms_control` media-perms bytes, with the viewer's `PERM_ALL` defaults) —
  with LLSD-XML build/parse helpers (`build_object_media_get_request` /
  `_update_request` / `_navigate_request`, `ObjectMediaResponse::from_llsd`).
  The field keys/verbs were cross-checked against the viewer's
  `llmediadataclient.cpp` / `llmediaentry.cpp` and OpenSim's `MoapModule`. Wired
  as
  `Command`/`SlCommand::{RequestObjectMedia, SetObjectMedia, NavigateObjectMedia}`
  through both runtimes: `RequestObjectMedia` POSTs an `ObjectMedia` GET and the
  reply is decoded by `Session::handle_caps_event` into
  `Event::ObjectMedia { object_id, version, faces: Vec<Option<MediaEntry>> }`;
  `SetObjectMedia` POSTs an `ObjectMedia` UPDATE and `NavigateObjectMedia` POSTs
  an `ObjectMediaNavigate` (both fire-and-forget — the sim advances the object's
  media version rather than replying, so a client re-fetches to observe). The
  two new caps (`CAP_OBJECT_MEDIA`, `CAP_OBJECT_MEDIA_NAVIGATE`) are added to
  the seed.
- **Parcel media control (UDP, receive-only).** Both `ParcelMediaCommandMessage`
  and `ParcelMediaUpdate` are `Trusted` (sim→viewer only), so this is a receive
  surface with no commands. A scripted `llParcelMediaCommandList` surfaces as
  `Event::ParcelMediaCommand { flags, command, time }` with a
  `ParcelMediaCommand` enum
  (Stop/Pause/Play/Loop/Texture/Url/Time/Agent/Unload/AutoAlign/Type/Size/
  Desc/LoopSet/`Other`, matching the viewer's `PARCEL_MEDIA_COMMAND_*`), and a
  parcel media-settings change surfaces as `Event::ParcelMediaUpdate` (a
  `ParcelMediaUpdateInfo`: media URL/id/auto-scale plus the extended MIME
  type/desc/width/height/loop). This complements the read-side parcel
  stream/media URLs added with #13 (the *static* `music_url`/`media_url` on
  `ParcelInfo`) — together a client now has the parcel's configured media *and*
  its live play/pause/seek control stream.

New value types `MediaEntry` (sl-wire), `ObjectMediaResponse` (sl-wire),
`ParcelMediaCommand`, `ParcelMediaUpdateInfo`, and the `MEDIA_PERM_*` constants,
all re-exported through both runtimes; the survey/example exhaustive event
matches updated. Covered by an `sl-wire` unit test (the per-face serialize →
`ObjectMediaResponse` parse round-trip, incl. the `undef` no-media slot and the
default-fill of an absent field) and three `lifecycle.rs` tests (the
`ParcelMediaCommandMessage` decode with the command enum, the
`ParcelMediaUpdate` decode incl. NUL-trimming, and the `ObjectMedia` CAPS GET
decode). *Live-verified against the local OpenSim (whose `MoapModule` serves
both caps) via the new `object_media` tokio example: rezzed a cube, set media on
face 0 over the `ObjectMedia` UPDATE cap, then fetched it back over the GET cap
— the reply decoded as 6 faces with media on face 0 (`current_url`, `auto_play`,
1024×512) and a `version` of `x-mv:0000000001/…`, the simulator's advanced media
version. `ObjectMediaNavigate` and the parcel-media receive path (which need a
scripted `llParcelMediaCommandList`) are unit-tested only. Test: local OpenSim —
no external stream needed to exercise the protocol (rendering the media itself
is out of protocol scope).*
