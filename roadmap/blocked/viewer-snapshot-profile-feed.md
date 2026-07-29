---
id: viewer-snapshot-profile-feed
title: Snapshot destination — profile feed
topic: viewer
status: blocked
origin: split from viewer-snapshot-floater (2026-07) — disk shipped first
blocked_by: [viewer-snapshot-floater, viewer-image-upload]
refs: [viewer-social-profiles, viewer-snapshot-to-inventory]
---

Context: [context/viewer.md](../context/viewer.md).

Add the **profile-feed** destination to the snapshot floater
([[viewer-snapshot-floater]]): post the captured snapshot to the agent's own
profile feed with a caption, the reference viewer's `Snapshot → Profile` panel.
The floater already owns the preview, the resolution / format controls and the
captured image; this task is the caption panel plus the post.

Unlike the postcard destination ([[viewer-snapshot-postcard]], whose
`SendPostcard` send path already exists), a profile-feed post first **uploads
the image as an asset** and then attaches it to a feed entry — so it is blocked
on the shared image-upload path ([[viewer-image-upload]]), the same uploader the
save-to-inventory destination ([[viewer-snapshot-to-inventory]]) feeds. The feed
write itself hangs off the profile surface ([[viewer-social-profiles]]).

Because it uploads, this destination carries the same resolution constraints and
(where applicable) cost confirmation as the inventory-texture path, not the
free-form disk save.

Reference (Firestorm, read-only): `panel_snapshot_profile.xml`,
`llpanelsnapshotprofile`, the profile-feed post capability.

Builds on: [[viewer-snapshot-floater]] (the floater and the captured image),
[[viewer-image-upload]] (the shared upload path),
[[viewer-social-profiles]] (the feed surface).
