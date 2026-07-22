---
id: viewer-profile-image-editing
title: Profile floater — set the profile / pick / classified images
topic: viewer
status: blocked
origin: user request (2026-07-22), while live-testing viewer-social-profiles
blocked_by: [viewer-ui-texture-picker]
refs: [viewer-social-profiles]
---

Context: [context/viewer.md](../context/viewer.md).

The profile floater ([[viewer-social-profiles]]) displays the profile
picture, the 1st-life picture, and the pick / classified snapshots, but
cannot **set** them — its saves keep the existing image ids. Once the
texture picker ([[viewer-ui-texture-picker]]) exists:

- make the own-profile image boxes (2nd Life picture, 1st Life picture) open
  the picker and carry the choice into `ProfileUpdate.image_id` /
  `fl_image_id` on Save (the reference's second-life picture texture
  control / image-actions menu);
- same for the pick editor's snapshot → `PickUpdate.snapshot_id` and the
  classified editor's snapshot → `ClassifiedUpdate.snapshot_id`.

All four update paths already carry the id — only the picker wiring is
missing.
