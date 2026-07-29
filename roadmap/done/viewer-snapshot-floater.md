---
id: viewer-snapshot-floater
title: Snapshot floater — preview, format, destinations
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-snapshot-tools
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-snapshot-quick-key, viewer-snapshot-to-inventory, viewer-360-snapshot, viewer-photo-hosting-upload, viewer-snapshot-postcard, viewer-snapshot-profile-feed]
---

Context: [context/viewer.md](../context/viewer.md).

Promote the debug `screenshot.rs` into a real **snapshot floater**
([[viewer-ui-widget-scaffold]]): a framed **preview** (refreshed on demand),
**include-UI / include-HUD** toggles, a **format** picker and a **save-to-disk**
destination laid out as **tabs**. Opened from the bottom toolbar's **Snapshot**
button (the reference's toolbar `snapshot` command, singular).

Shipped (`src/snapshot_floater.rs`):

- **Refresh, not a live feed.** Like the reference, a Refresh button (or a save)
  takes one shot on demand — the **actual primary window**, read back with
  `Screenshot::primary_window`, so it is guaranteed to match the on-screen frame
  (the same tone-mapped, environment-lit image). An early attempt at a live
  second-camera preview was dropped: it rendered dark because it did not carry
  the main camera's probe-generated `EnvironmentMapLight` (image-based
  lighting), and re-deriving the whole pipeline off-screen is exactly the
  fragility the window-capture approach sidesteps.
- **Include UI / Include HUD.** Because the window is captured, the two toggles
  fall out naturally: include-UI off (default) hides the whole UI (`UiRoot`, via
  `Display::None`) for the shot frame; include-HUD off (default) hides the
  worn-HUD attachment subtree (`HudScreen`, via `Visibility::Hidden`) — not the
  HUD camera, which the UI renders through and which shares the world camera's
  HDR chain. A tiny hide → wait-a-frame → shoot → restore state machine, with
  the restore on a timer (not the shot callback) so the UI can never be stranded
  hidden.
- **Save to disk.** Writes the captured frame at the **window's own resolution**
  (free-form disk output) in the picked format (PNG / JPEG / BMP / TGA) to the
  platform Pictures folder, echoing the saved path to nearby chat — the running
  local-chat index photographers rely on, matching the quick key
  ([[viewer-snapshot-quick-key]]). Include-UI / include-HUD / format persist per
  avatar.
- **Destination tabs.** Save to Disk is a live tab; Postcard / Profile /
  Inventory are placeholder tabs pointing at their follow-up tasks.

Split out to their own follow-up tasks (the reason this floater ships with disk
first): the **postcard / e-mail** destination is [[viewer-snapshot-postcard]]
(its `SendPostcard` path already exists), the **profile-feed** destination is
[[viewer-snapshot-profile-feed]] (it uploads, so it waits on the shared
image-upload path). **Save-to-inventory as a texture** is
[[viewer-snapshot-to-inventory]] (power-of-two / L$ rules),
**equirectangular 360 capture** is [[viewer-360-snapshot]] (a distinct
capture-renderer), and sharing to external sites is
[[viewer-photo-hosting-upload]]. Each downscales the captured frame to its own
constraints and plugs into this floater once landed; disk does not downscale.

Reference (Firestorm, read-only): `llsnapshotlivepreview`, `llfloatersnapshot`,
`panel_snapshot_*`, `llviewerassetupload`.

Builds on: `screenshot.rs` and the asset-upload caps (`upload.rs`).
