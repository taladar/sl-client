---
id: viewer-parcel-options-access-media
title: About Land floater — access / ban / media / sound
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-parcel-options
blocked_by: [viewer-parcel-options-general]
---

Context: [context/viewer.md](../context/viewer.md).

The "About Land" floater, second half: the **access** and **ban** list tabs, the
**media** tab and the **sound** / audio tab, with their edits. Builds on the
floater shell and general tabs from [[viewer-parcel-options-general]], adding
the access-control and media/audio panels and the `ParcelPropertiesUpdate`
writes for each.

Reference (Firestorm, read-only): `llfloaterland`, `llpanellandaudio`,
`llpanellandmedia`; the `ParcelPropertiesUpdate` message.

Builds on: `protocol-13` parcel — rich parcel data arrives over the CAPS event
queue, not UDP.

Deps: [[viewer-parcel-options-general]].

## Done (2026-07-28)

Implemented alongside [[viewer-parcel-options-general]] as part of the whole
nine-tab About Land floater (`sl-client-bevy-viewer/src/about_land.rs`):

- **Access** — the public-access / payment / age / group / passes checkboxes and
  pass price / hours (via `ParcelFlags` + `ParcelUpdate`), plus the **Allowed**
  and **Banned** resident lists as bounded, scrolling **table widgets** with a
  per-row Remove and an Add (avatar picker), writing `UpdateParcelAccessList`.
- **Media** — media URL, replace-texture (picker) and auto-scale are editable
  (`ParcelUpdate`); media type / size / loop are shown **read-only** (no write
  path in `ParcelUpdate` — see [[viewer-parcel-config-missing-writes]]).
- **Sound** — music URL, restrict-sounds-to-parcel, and voice enable / restrict
  are editable; the avatar-sound toggles are shown **read-only** (no write
  path).

The estate voice-channel URI / `RequestParcelVoiceInfo` read-out and per-face
media were out of scope. The remaining non-writable controls are tracked in
[[viewer-parcel-config-missing-writes]].
