---
id: viewer-parcel-options-access-media
title: About Land floater — access / ban / media / sound
topic: viewer
status: blocked
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
