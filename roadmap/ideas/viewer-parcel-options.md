---
id: viewer-parcel-options
title: Parcel option viewing & editing
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-framework]
---

Context: [context/viewer.md](../context/viewer.md).

The "About Land" floater: view and edit parcel general info, covenant, objects,
options, media & audio, access & ban lists, and sound.

Reference (Firestorm, read-only): `llfloaterland`, `llpanelland`,
`llpanellandaudio`, `llpanellandmedia`; the `ParcelPropertiesUpdate` message.

Builds on: `protocol-13` parcel — note the known reality that rich parcel /
region data arrives over the CAPS event queue, not UDP.

Deps: [[viewer-ui-framework]].
