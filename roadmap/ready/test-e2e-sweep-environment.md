---
id: test-e2e-sweep-environment
title: End-to-end tests for the environment editors and their gates
topic: test
status: ready
origin: test-e2e-live-verify-sweep (2026-09-30)
points: 8
refs: [test-e2e-live-verify-sweep]
---

Context: [context/automation.md](../context/automation.md).

The environment windows were each unit-tested and never driven against a
grid. The fake grid serves `ExtEnvironment`, `UpdateSettingsAgentInventory`
and the settings assets, so each becomes a fake-grid test: open, edit, save,
and read back what the grid stored (and what the sky readout says).

- [[viewer-environment-fixed-editor]]: open, edit and save a sky / water
  settings item (`UpdateSettingsAgentInventory` plus the flags stamp).
- [[viewer-environment-day-cycle-editor]]: open, scrub, add a keyframe,
  save.
- [[viewer-environment-land-day-cycle-edit]]: Customize Day Cycle, edit a
  keyframe, Save, Apply — the inline cycle OpenSim uses.
- [[viewer-region-environment-panel]]: publish with region scope, day
  length and offset, then reset (aditi's per-track `trackno` path belongs
  to [[test-e2e-sweep-live-grid]]).
- [[viewer-environment-import-legacy-presets]] and
  [[viewer-windlight-bulk-import]]: import a `windlight/skies/*.xml` and a
  folder of them. Both go through the desktop's file chooser, so a test
  needs a way to answer it with a path (the chooser's own seam, not a
  second code path).
- [[viewer-environment-my-environments]]: drive the picker.
- [[viewer-environment-settings-unsupported-gate]]: a fake region that
  serves neither settings capability greys the entries — the one grid that
  can show it, since aditi and OpenSim both serve both.
- [[viewer-environment-personal-lighting]] and
  [[viewer-ui-virtual-trackball]]: the pickers, the fade, the trackball,
  and the parcel layer when walking between two parcels.
