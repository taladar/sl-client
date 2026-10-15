---
id: viewer-snapshot-profile-feed
title: Snapshot destination — profile feed
topic: viewer
status: ready
origin: split from viewer-snapshot-floater (2026-07) — disk shipped first
blocked_by: [viewer-snapshot-floater]
refs: [viewer-social-profiles, viewer-snapshot-to-inventory,
  viewer-image-upload]
---

Context: [context/viewer.md](../context/viewer.md).

Add the **profile-feed** destination to the snapshot floater
([[viewer-snapshot-floater]]): post the captured snapshot to the agent's own
profile feed with a caption, the reference viewer's `Snapshot → Profile` panel.
The floater already owns the preview, the resolution / format controls and the
captured image; this task is the caption panel plus the post.

Unlike the postcard destination ([[viewer-snapshot-postcard]]), a
profile-feed post does **not** go through the asset uploader. Audit
2026-10-15 against `llwebprofile.cpp`: the reference uploads to the
**web profile** service, not to the grid's asset store —
`GET <profile url>snapshots/s3_upload_config` for an upload URL and form
fields, a multipart `POST` of the JPEG (with caption and, optionally, the
location) to that URL, then a `GET` of the redirect it answers with. No
`NewFileAgentInventory`, no L$ cost, no power-of-two constraint. So the
earlier `blocked_by: viewer-image-upload` edge was wrong and has been
dropped; the upload wizard ([[viewer-image-upload]]) is unrelated.

What this needs instead: the web-profile base URL (the reference's
`getProfileURL`, from the login response / grid info), the cookie-
authenticated HTTP round trip, and a "not available on this grid" state
where there is no web profile (OpenSim). Confirm first that Second Life's
web-profile upload endpoint still answers — the in-viewer profiles of
[[viewer-social-profiles]] replaced most of the web profile.

Reference (Firestorm, read-only): `panel_snapshot_profile.xml`,
`llpanelsnapshotprofile`, `llwebprofile.cpp` (the upload round trip).

Builds on: [[viewer-snapshot-floater]] (the floater and the captured image),
[[viewer-social-profiles]] (the feed surface).
