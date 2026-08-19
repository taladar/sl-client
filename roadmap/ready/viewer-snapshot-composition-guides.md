---
id: viewer-snapshot-composition-guides
title: Snapshot composition guides, capture frame & filename patterns
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-snapshot-floater, viewer-phototools]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's photo framing aids: an aspect-ratio capture frame masking
the screen outside the capture region (`FSSnapshotShowCaptureFrame`),
composition guide overlays — rule of thirds and other styles — drawn
over the world while framing a shot, with configurable style, colour and
line width (`FSSnapshotShowGuides`, `FSSnapshotGuideStyle`,
`FSSnapshotFrame*` settings), plus a small settings floater that
configures them (`floater_snapshot_guide_settings.xml`). On the file
side it also offers filename patterns: timestamped local names
(`FSSnapshotLocalNamesWithTimestamps`) and a per-account base-name
pattern (`SnapshotBaseName`).

Our snapshot floater (`sl-client-bevy-viewer/src/snapshot_floater.rs`,
[[viewer-snapshot-floater]] done) has neither framing guides nor
filename patterns. Implementing this means a screen-space overlay
(frame mask + guide lines matching the pending capture aspect), the
guide-settings surface (a small floater or a snapshot-floater section),
and the local-save filename pattern options; guide preferences persist
in viewer settings. Natural companion to [[viewer-phototools]].

Reference (Firestorm, read-only): `indra/newview/llfloatersnapshot.cpp`,
`indra/newview/llsnapshotlivepreview.cpp`,
`indra/newview/skins/default/xui/en/floater_snapshot_guide_settings.xml`.
