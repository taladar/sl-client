---
id: viewer-stream-favorites
title: Audio-stream favorites + now-playing title floater
topic: viewer
status: blocked
origin: debug-settings/chat-lines survey (2026-07-23)
blocked_by: [viewer-streaming-audio]
---

Context: [context/viewer.md](../context/viewer.md).

Two Firestorm stream conveniences on top of the parcel-audio player:

- **Stream favorites** (`FSStreamList`): a user-managed list of saved
  stream URLs with add/remove/play — playing one overrides the parcel
  stream until cleared; plus the parcel-music autoplay policy
  (`FSParcelMusicAutoPlay`) and fade-in/out on switch
  (`FSFadeAudioStream`, `FSAudioMusicFadeIn`/`Out`).
- **Stream Title floater** (World ▸ Stream Title, `fs_streamtitle`): a
  compact always-on-top readout of the current stream's now-playing
  metadata, updating on title change, with an optional chat echo of
  title changes. The ICY metadata is already parsed by the audio
  backend (`sl-gst/src/stream.rs`).

Scope: the persisted favorites model + management UI, the
override-parcel-stream playback path, the title floater, and the
optional title-change chat notice.

Reference (Firestorm, read-only): `FSStreamList` setting,
`fs_streamtitle` floater (`Floater.Toggle fs_streamtitle`,
`menu_viewer.xml` World section).

Builds on: the streaming-audio player (in progress) — both pieces sit
directly on its stream-selection and metadata surface.
