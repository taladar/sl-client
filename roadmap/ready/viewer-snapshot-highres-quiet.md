---
id: viewer-snapshot-highres-quiet
title: High-resolution snapshot capture + quiet-snapshot mode
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-snapshot-floater, viewer-snapshot-quick-key,
  viewer-360-snapshot, viewer-qol-toggles]
---

Context: [context/viewer.md](../context/viewer.md).

Two Advanced-menu snapshot toggles the reference offers that our
snapshot pipeline lacks. **High-res Snapshot** (`HighResSnapshot`,
menu_viewer.xml L3115–3138) multiplies the capture resolution relative
to the window: the reference renders the frame off-screen at the
scaled size so disk saves can exceed screen resolution. Our snapshot
floater and quick-key ([[viewer-snapshot-floater]] and
[[viewer-snapshot-quick-key]], both done) save only at the window's
own resolution (`sl-client-bevy-viewer/src/snapshot_floater.rs` states
this explicitly in its header). Scope: an off-screen render-target
capture at a selectable multiplier, and/or free custom dimensions in
the snapshot floater, plumbed through the existing capture path.

**Quiet Snapshots** (`QuietSnapshotsToDisk`) suppresses the shutter
sound and the camera-click ViewerEffect/animation broadcast, so
photographers don't spam a scene with click noises and camera poses on
every capture. Both are settings-store toggles surfaced in the
snapshot floater and/or the Advanced menu (the toggle family sits next
to the [[viewer-qol-toggles]] cluster).

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_viewer.xml` (L3115–3138),
`indra/newview/llviewerwindow.cpp` (saveSnapshot, `HighResSnapshot`
handling), `indra/newview/llfloatersnapshot.cpp`.
