---
id: viewer-about-release-notes
title: Link the simulator's release notes from the About floater
topic: viewer
status: ready
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities]
---

Context: [context/viewer.md](../context/viewer.md).

Both grids grant `ServerReleaseNotes`. Firestorm GETs it with redirects off and
reads the `Location:` header as the release-notes URL, shown in the About
floater. Our About floater shows the simulator version only. Fetch the URL and
add the link.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.
